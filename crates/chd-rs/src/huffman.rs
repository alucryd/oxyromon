//! Huffman coding, decoder and encoder sides.
//!
//! A port of MAME's `huffman_decoder` and `huffman_encoder`
//! (`src/lib/util/huffman.h` and `src/lib/util/huffman.cpp`): canonical
//! Huffman codes with a flat lookup table and two tree encodings, plain
//! RLE and Huffman-coded RLE.

use crate::bitstream::{BitstreamIn, BitstreamOut};
use crate::error::{Error, Result};

/// A Huffman decoder for `_num_codes` symbols with codes up to
/// `_max_bits` bits long.
pub(crate) struct HuffmanDecoder {
    num_codes: usize,
    max_bits: u32,
    /// Code length of each symbol, 0 when unused.
    node_bits: Vec<u8>,
    /// Canonical code of each symbol.
    node_code: Vec<u32>,
    /// Lookup table mapping `_max_bits` peeked bits to
    /// `(code << 5) | code length`.
    lookup: Vec<u16>,
}

impl HuffmanDecoder {
    pub fn new(num_codes: usize, max_bits: u32) -> Self {
        assert!(max_bits > 0 && max_bits <= 16);
        Self {
            num_codes,
            max_bits,
            node_bits: vec![0; num_codes],
            node_code: vec![0; num_codes],
            lookup: vec![0; 1 << max_bits],
        }
    }

    /// Number of bits per tree entry, derived from the maximum code
    /// length.
    fn tree_bits(&self) -> u32 {
        if self.max_bits >= 16 {
            5
        } else if self.max_bits >= 8 {
            4
        } else {
            3
        }
    }

    /// Imports an RLE-encoded Huffman tree from a source data stream.
    pub fn import_tree_rle(&mut self, bitbuf: &mut BitstreamIn<'_>) -> Result<()> {
        let numbits = self.tree_bits();

        // Loop until we read all the nodes.
        let mut curnode = 0;
        while curnode < self.num_codes {
            // A non-one value is just raw.
            let nodebits = bitbuf.read(numbits);
            if nodebits != 1 {
                self.node_bits[curnode] = nodebits as u8;
                curnode += 1;
            }
            // A one value is an escape code.
            else {
                // A double 1 is just a single 1.
                let nodebits = bitbuf.read(numbits);
                if nodebits == 1 {
                    self.node_bits[curnode] = 1;
                    curnode += 1;
                }
                // Otherwise, we need one more value for the repeat
                // count.
                else {
                    let repcount = bitbuf.read(numbits) + 3;
                    if curnode + repcount as usize > self.num_codes {
                        return Err(Error::Corrupt("invalid huffman tree".to_string()));
                    }
                    for _ in 0..repcount {
                        self.node_bits[curnode] = nodebits as u8;
                        curnode += 1;
                    }
                }
            }
        }

        self.finish_import(bitbuf)
    }

    /// Imports a Huffman-encoded Huffman tree from a source data stream.
    pub fn import_tree_huffman(&mut self, bitbuf: &mut BitstreamIn<'_>) -> Result<()> {
        // Start by parsing the lengths for the small tree.
        let mut smallhuff = Self::new(24, 6);
        smallhuff.node_bits[0] = bitbuf.read(3) as u8;
        let start = bitbuf.read(3) as usize + 1;
        let mut count = 0;
        for index in 1..24 {
            if index < start || count == 7 {
                smallhuff.node_bits[index] = 0;
            } else {
                count = bitbuf.read(3);
                smallhuff.node_bits[index] = (if count == 7 { 0 } else { count }) as u8;
            }
        }

        // Then regenerate the tree.
        smallhuff.assign_canonical_codes()?;
        smallhuff.build_lookup_table()?;

        // Determine the maximum length of an RLE count.
        let mut temp = self.num_codes.saturating_sub(9);
        let mut rlefullbits = 0;
        while temp != 0 {
            temp >>= 1;
            rlefullbits += 1;
        }

        // Now process the rest of the data.
        let mut last = 0;
        let mut curcode = 0;
        while curcode < self.num_codes {
            let value = smallhuff.decode_one(bitbuf);
            if value != 0 {
                last = value - 1;
                self.node_bits[curcode] = last as u8;
                curcode += 1;
            } else {
                let mut count = bitbuf.read(3) + 2;
                if count == 7 + 2 {
                    count += bitbuf.read(rlefullbits);
                }
                while count != 0 && curcode < self.num_codes {
                    self.node_bits[curcode] = last as u8;
                    curcode += 1;
                    count -= 1;
                }
            }
        }

        self.finish_import(bitbuf)
    }

    /// Shared tail of both tree imports: assign canonical codes, build
    /// the lookup table and check for a short input.
    fn finish_import(&mut self, bitbuf: &mut BitstreamIn<'_>) -> Result<()> {
        self.assign_canonical_codes()?;
        self.build_lookup_table()?;
        if bitbuf.overflow() {
            return Err(Error::Corrupt("truncated huffman tree".to_string()));
        }
        Ok(())
    }

    /// Assigns canonical codes to all nodes based on their lengths.
    fn assign_canonical_codes(&mut self) -> Result<()> {
        // Build up a histogram of bit lengths.
        let mut bithisto = [0u32; 33];
        for curcode in 0..self.num_codes {
            let numbits = self.node_bits[curcode] as u32;
            if numbits > self.max_bits {
                return Err(Error::Corrupt("huffman code too long".to_string()));
            }
            bithisto[numbits as usize] += 1;
        }

        // For each code length, determine the starting code number.
        let mut curstart = 0;
        for codelen in (1..=32).rev() {
            let nextstart = (curstart + bithisto[codelen]) >> 1;
            if codelen != 1 && nextstart * 2 != curstart + bithisto[codelen] {
                return Err(Error::Corrupt("inconsistent huffman tree".to_string()));
            }
            bithisto[codelen] = curstart;
            curstart = nextstart;
        }

        // Now assign canonical codes.
        for curcode in 0..self.num_codes {
            let numbits = self.node_bits[curcode];
            if numbits > 0 {
                self.node_code[curcode] = bithisto[numbits as usize];
                bithisto[numbits as usize] += 1;
            }
        }
        Ok(())
    }

    /// Builds the lookup table for fast decoding.
    fn build_lookup_table(&mut self) -> Result<()> {
        for curcode in 0..self.num_codes {
            let numbits = self.node_bits[curcode] as u32;
            if numbits == 0 {
                continue;
            }
            // The canonical code must fit its length; an over-full
            // tree would overflow the lookup table.
            if self.node_code[curcode] >> numbits != 0 {
                return Err(Error::Corrupt("inconsistent huffman tree".to_string()));
            }
            let value = ((curcode as u32) << 5 | numbits) as u16;

            // Fill all matching entries.
            let shift = self.max_bits - numbits;
            let start = (self.node_code[curcode] << shift) as usize;
            let end = (((self.node_code[curcode] + 1) << shift) - 1) as usize;
            for slot in &mut self.lookup[start..=end] {
                *slot = value;
            }
        }
        Ok(())
    }

    /// Decodes a single code from the Huffman stream.
    pub fn decode_one(&mut self, bitbuf: &mut BitstreamIn<'_>) -> u32 {
        // Peek ahead to get maxbits worth of data, look it up, then
        // remove the actual number of bits for this code.
        let bits = bitbuf.peek(self.max_bits);
        let lookup = self.lookup[bits as usize];
        bitbuf.remove(u32::from(lookup & 0x1f));

        // Return the value.
        lookup as u32 >> 5
    }
}

/// A node of a Huffman tree under construction.
#[derive(Clone, Copy)]
struct Node {
    /// Index of the parent node, [`usize::MAX`] when unconnected.
    parent: usize,
    weight: u32,
    bits: u32,
    numbits: u32,
}

impl Node {
    const EMPTY: Self = Self {
        parent: usize::MAX,
        weight: 0,
        bits: 0,
        numbits: 0,
    };
}

/// A Huffman encoder for `num_codes` symbols with codes up to
/// `max_bits` bits long.
///
/// Unlike the decoder, the encoder keeps the whole tree: symbols are
/// tallied with [`HuffmanEncoder::histo_one`], the tree is built with
/// [`HuffmanEncoder::compute_tree_from_histo`], exported with
/// [`HuffmanEncoder::export_tree_rle`] or
/// [`HuffmanEncoder::export_tree_huffman`], and each symbol is written
/// with [`HuffmanEncoder::encode_one`].
pub(crate) struct HuffmanEncoder {
    num_codes: usize,
    max_bits: u32,
    /// Occurrences of each symbol seen since the last reset.
    histo: Vec<u32>,
    /// Leaf nodes, then the interior nodes built on top of them.
    nodes: Vec<Node>,
}

impl HuffmanEncoder {
    pub fn new(num_codes: usize, max_bits: u32) -> Self {
        assert!(max_bits > 0 && max_bits <= 24);
        Self {
            num_codes,
            max_bits,
            histo: vec![0; num_codes],
            nodes: vec![Node::EMPTY; num_codes * 2],
        }
    }

    /// Tallies one occurrence of a symbol.
    pub fn histo_one(&mut self, symbol: usize) {
        self.histo[symbol] += 1;
    }

    /// Writes a single code to the Huffman stream.
    pub fn encode_one(&self, bitbuf: &mut BitstreamOut<'_>, symbol: usize) {
        let node = self.nodes[symbol];
        bitbuf.write(node.bits, node.numbits);
    }

    /// Builds a Huffman tree from the collected histogram, scaling the
    /// symbol weights down until every code fits in `max_bits`.
    pub fn compute_tree_from_histo(&mut self) -> Result<()> {
        let mut sdatacount = 0u32;
        for count in &self.histo {
            sdatacount = sdatacount.wrapping_add(*count);
        }

        // Binary search for the smallest weight scale, relative to
        // the total data count, that produces codes short enough to
        // fit; the search starts with the unscaled weights, which are
        // exactly half of the upper bound.
        let mut lowerweight = 0u32;
        let mut upperweight = sdatacount.wrapping_mul(2);
        loop {
            let curweight = lowerweight.wrapping_add(upperweight) / 2;
            let curmaxbits = self.build_tree(sdatacount, curweight);
            if curmaxbits <= self.max_bits {
                lowerweight = curweight;
                if curweight == sdatacount || upperweight.wrapping_sub(lowerweight) <= 1 {
                    break;
                }
            } else {
                upperweight = curweight;
            }
        }

        self.assign_canonical_codes()
    }

    /// Builds a Huffman tree from the histogram, scaling each symbol
    /// weight to `weight = histo * totalweight / totaldata`, and
    /// returns the length of the longest code.
    fn build_tree(&mut self, totaldata: u32, totalweight: u32) -> u32 {
        // Reset the leaf nodes; interior nodes are initialised as
        // they are created.
        for node in self.nodes.iter_mut().take(self.num_codes) {
            *node = Node::EMPTY;
        }

        // Build a list of the nodes to merge, sorted by descending
        // weight and then by ascending symbol.
        let mut list: Vec<usize> = Vec::with_capacity(self.num_codes * 2);
        for curcode in 0..self.num_codes {
            let histo = self.histo[curcode];
            if histo != 0 {
                list.push(curcode);
                let node = &mut self.nodes[curcode];
                node.bits = curcode as u32;
                node.weight = ((u64::from(histo) * u64::from(totalweight) / u64::from(totaldata))
                    as u32)
                    .max(1);
            }
        }
        list.sort_by(|&a, &b| {
            self.nodes[b]
                .weight
                .cmp(&self.nodes[a].weight)
                .then(self.nodes[a].bits.cmp(&self.nodes[b].bits))
        });

        // Merge the two least weight nodes repeatedly, keeping the
        // merged parents in sorted position; with equal weights the
        // fresh node lands after the existing ones.
        let mut nextalloc = self.num_codes;
        while list.len() > 1 {
            let node1 = list.pop().unwrap();
            let node0 = list.pop().unwrap();
            let newnode = nextalloc;
            nextalloc += 1;
            self.nodes[newnode] = Node::EMPTY;
            self.nodes[node0].parent = newnode;
            self.nodes[node1].parent = newnode;
            let newweight = self.nodes[node0]
                .weight
                .wrapping_add(self.nodes[node1].weight);
            self.nodes[newnode].weight = newweight;
            let position = list
                .iter()
                .position(|&item| newweight > self.nodes[item].weight)
                .unwrap_or(list.len());
            list.insert(position, newnode);
        }

        // Walk each used leaf up to the root to get its code length,
        // clamping the lone root, if any, to a single bit.
        let mut maxbits = 0;
        for curcode in 0..self.num_codes {
            if self.nodes[curcode].weight > 0 {
                let mut numbits = 0;
                let mut curnode = curcode;
                while self.nodes[curnode].parent != usize::MAX {
                    numbits += 1;
                    curnode = self.nodes[curnode].parent;
                }
                let node = &mut self.nodes[curcode];
                node.numbits = numbits.max(1);
                maxbits = maxbits.max(node.numbits);
            }
        }
        maxbits
    }

    /// Assigns canonical codes to the current tree.
    fn assign_canonical_codes(&mut self) -> Result<()> {
        // Count the code lengths, checking that they fit.
        let mut bithisto = [0u32; 33];
        for curcode in 0..self.num_codes {
            let numbits = self.nodes[curcode].numbits;
            if numbits > self.max_bits {
                return Err(Error::Compression(
                    "huffman code lengths exceed the maximum".to_string(),
                ));
            }
            bithisto[numbits as usize] += 1;
        }

        // Walk backwards to determine the starting code for each bit
        // length, checking that the tree is complete.
        let mut curstart = 0u32;
        for codelen in (1..=32).rev() {
            let nextstart = (curstart + bithisto[codelen]) >> 1;
            if codelen != 1 && nextstart * 2 != curstart + bithisto[codelen] {
                return Err(Error::Compression("inconsistent huffman tree".to_string()));
            }
            bithisto[codelen] = curstart;
            curstart = nextstart;
        }

        // Assign the codes in order.
        for curcode in 0..self.num_codes {
            let numbits = self.nodes[curcode].numbits;
            if numbits > 0 {
                self.nodes[curcode].bits = bithisto[numbits as usize];
                bithisto[numbits as usize] += 1;
            }
        }
        Ok(())
    }

    /// Exports the current tree with plain run-length encoding, the
    /// format [`HuffmanDecoder::import_tree_rle`] reads.
    pub fn export_tree_rle(&self, bitbuf: &mut BitstreamOut<'_>) -> Result<()> {
        let numbits = if self.max_bits >= 16 {
            5
        } else if self.max_bits >= 8 {
            4
        } else {
            3
        };

        // Walk the code lengths, grouping consecutive runs.
        let mut lastval: i32 = -1;
        let mut repcount: i32 = 0;
        for curcode in 0..self.num_codes {
            let newval = self.nodes[curcode].numbits as i32;
            if newval == lastval {
                repcount += 1;
            } else {
                if repcount != 0 {
                    Self::write_rle_tree_bits(bitbuf, lastval, repcount, numbits);
                }
                lastval = newval;
                repcount = 1;
            }
        }
        Self::write_rle_tree_bits(bitbuf, lastval, repcount, numbits);

        if bitbuf.overflow() {
            return Err(Error::Compression(
                "huffman tree does not fit in the output buffer".to_string(),
            ));
        }
        Ok(())
    }

    /// Writes a run of identical code lengths, escaping runs longer
    /// than two with the all-ones pattern.
    fn write_rle_tree_bits(
        bitbuf: &mut BitstreamOut<'_>,
        value: i32,
        mut repcount: i32,
        numbits: u32,
    ) {
        while repcount > 0 {
            if value == 1 {
                // Write the length twice, 1 is the escape value.
                bitbuf.write(1, numbits);
                bitbuf.write(1, numbits);
                repcount -= 1;
            } else if repcount <= 2 {
                // If we only have 1 or 2 left, just write them out.
                bitbuf.write(value as u32, numbits);
                repcount -= 1;
            } else {
                // Otherwise write as many as we can.
                let cur_reps = (repcount - 3).min((1 << numbits) - 1);
                bitbuf.write(1, numbits);
                bitbuf.write(value as u32, numbits);
                bitbuf.write(cur_reps as u32, numbits);
                repcount -= cur_reps + 3;
            }
        }
    }

    /// Exports the current tree with Huffman-coded run-length
    /// encoding, the format [`HuffmanDecoder::import_tree_huffman`]
    /// reads.
    pub fn export_tree_huffman(&self, bitbuf: &mut BitstreamOut<'_>) -> Result<()> {
        // Small codes for the RLE encoding: the code lengths plus
        // one, with zero as the run token.
        let mut rle_data: Vec<u8> = Vec::with_capacity(self.num_codes);
        let mut rle_lengths: Vec<u16> = Vec::with_capacity(self.num_codes / 3);
        let mut smallhuff = Self::new(24, 6);

        // Walk the code lengths, grouping consecutive runs.
        let mut last: i32 = -1;
        let mut repcount: i32 = 0;
        for curcode in 0..self.num_codes {
            let newval = self.nodes[curcode].numbits as i32;
            if newval != last && repcount > 0 {
                if repcount == 1 {
                    // Just one, write it as a literal.
                    smallhuff.histo_one((last + 1) as usize);
                    rle_data.push((last + 1) as u8);
                } else {
                    // Two or more, write the run token and length.
                    smallhuff.histo_one(0);
                    rle_data.push(0);
                    rle_lengths.push((repcount - 2) as u16);
                }
            }
            if newval == last {
                repcount += 1;
            } else {
                smallhuff.histo_one((newval + 1) as usize);
                rle_data.push((newval + 1) as u8);
                last = newval;
                repcount = 0;
            }
        }
        if repcount > 0 {
            if repcount == 1 {
                smallhuff.histo_one((last + 1) as usize);
                rle_data.push((last + 1) as u8);
            } else {
                smallhuff.histo_one(0);
                rle_data.push(0);
                rle_lengths.push((repcount - 2) as u16);
            }
        }

        // Build the tree for the small codes.
        smallhuff.compute_tree_from_histo()?;

        // Determine the first and last non-zero entries, capped at
        // eight, since shorter lengths are cheaper to enumerate.
        let mut first_non_zero = 31usize;
        let mut last_non_zero = 0usize;
        for index in 1..24 {
            if smallhuff.nodes[index].numbits != 0 {
                if first_non_zero == 31 {
                    first_non_zero = index;
                }
                last_non_zero = index;
            }
        }
        first_non_zero = first_non_zero.min(8);

        // Write out the small tree.
        bitbuf.write(smallhuff.nodes[0].numbits, 3);
        bitbuf.write((first_non_zero - 1) as u32, 3);
        for index in first_non_zero..=last_non_zero {
            bitbuf.write(smallhuff.nodes[index].numbits, 3);
        }
        bitbuf.write(7, 3);

        // Determine the maximum length of an RLE count.
        let mut temp = self.num_codes.saturating_sub(9);
        let mut rlefullbits = 0;
        while temp != 0 {
            temp >>= 1;
            rlefullbits += 1;
        }

        // Encode the RLE data using the small tree.
        let mut lengths_index = 0;
        for &data in &rle_data {
            smallhuff.encode_one(bitbuf, usize::from(data));
            if data == 0 {
                let count = rle_lengths[lengths_index];
                lengths_index += 1;
                if count < 7 {
                    bitbuf.write(u32::from(count), 3);
                } else {
                    bitbuf.write(7, 3);
                    bitbuf.write(u32::from(count - 7), rlefullbits);
                }
            }
        }

        if bitbuf.overflow() {
            return Err(Error::Compression(
                "huffman tree does not fit in the output buffer".to_string(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn import_rle_raw_lengths() {
        // 0b0010 repeated: four symbols of two bits each.
        let data = [0b0010_0010, 0b0010_0010];
        let mut stream = BitstreamIn::new(&data);
        let mut huff = HuffmanDecoder::new(4, 8);
        huff.import_tree_rle(&mut stream).unwrap();
        assert_eq!(huff.node_bits[..], [2, 2, 2, 2]);
        // Canonical codes 00, 01, 10, 11.
        assert_eq!(huff.decode_one(&mut stream), 0);
        assert_eq!(huff.decode_one(&mut stream), 0);
    }

    #[test]
    fn import_rle_with_escape() {
        // Literal 4, then escape value 4 with repeat count 4 + 3 = 7.
        let data = [0b0100_0001, 0b0100_0100];
        let mut stream = BitstreamIn::new(&data);
        let mut huff = HuffmanDecoder::new(8, 8);
        huff.import_tree_rle(&mut stream).unwrap();
        assert_eq!(huff.node_bits[..], [4, 4, 4, 4, 4, 4, 4, 4]);

        // Canonical codes 0b0000..0b0111 for symbols 0..7.
        let codes = [0b0100_0101, 0b0110_0111, 0b0000_0001, 0b0010_0011];
        let mut stream = BitstreamIn::new(&codes);
        for expected in [4, 5, 6, 7, 0, 1, 2, 3] {
            assert_eq!(huff.decode_one(&mut stream), expected);
        }
    }

    #[test]
    fn import_rle_double_one_is_literal_one() {
        // 0001 0001 is a literal length of 1, not an escape.
        let data = [0b0001_0001, 0b0001_0001];
        let mut stream = BitstreamIn::new(&data);
        let mut huff = HuffmanDecoder::new(2, 8);
        huff.import_tree_rle(&mut stream).unwrap();
        assert_eq!(huff.node_bits[..], [1, 1]);

        // Complete tree: 0 decodes symbol 0, 1 decodes symbol 1.
        let codes = [0b0100_0000];
        let mut stream = BitstreamIn::new(&codes);
        assert_eq!(huff.decode_one(&mut stream), 0);
        assert_eq!(huff.decode_one(&mut stream), 1);
    }

    #[test]
    fn overfull_tree_is_rejected() {
        // Eight symbols of one bit each: MAME would scribble past its
        // lookup table, we reject it instead.
        let data = [0b0001_0001, 0b0001_0001, 0b0001_0001, 0b0001_0001];
        let mut stream = BitstreamIn::new(&data);
        let mut huff = HuffmanDecoder::new(8, 8);
        assert!(matches!(
            huff.import_tree_rle(&mut stream),
            Err(Error::Corrupt(_))
        ));
    }

    #[test]
    fn incomplete_tree_is_rejected() {
        // Five symbols of four bits each breaks the Kraft parity.
        let data = [0b0100_0100, 0b0100_0100, 0b0100_0000];
        let mut stream = BitstreamIn::new(&data);
        let mut huff = HuffmanDecoder::new(5, 8);
        assert!(matches!(
            huff.import_tree_rle(&mut stream),
            Err(Error::Corrupt(_))
        ));
    }

    #[test]
    fn truncated_tree_is_rejected() {
        let data = [0b0010_0010];
        let mut stream = BitstreamIn::new(&data);
        let mut huff = HuffmanDecoder::new(8, 8);
        assert!(matches!(
            huff.import_tree_rle(&mut stream),
            Err(Error::Corrupt(_))
        ));
    }

    /// Builds a tree from a code-length vector, exports it with plain
    /// RLE and returns the lengths the decoder read back. The lengths
    /// must form a valid tree, the decoder checks.
    fn rle_roundtrip_lengths(num_codes: usize, max_bits: u32, lengths: &[u32]) -> Vec<u32> {
        let mut enc = HuffmanEncoder::new(num_codes, max_bits);
        for (code, &length) in lengths.iter().enumerate() {
            enc.nodes[code].numbits = length;
        }
        let mut out = vec![0u8; 4096];
        let mut bitbuf = BitstreamOut::new(&mut out);
        enc.export_tree_rle(&mut bitbuf).unwrap();
        let written = bitbuf.flush();

        let mut dec = HuffmanDecoder::new(num_codes, max_bits);
        let mut stream = BitstreamIn::new(&out[..written]);
        dec.import_tree_rle(&mut stream).unwrap();
        (0..num_codes)
            .map(|index| u32::from(dec.node_bits[index]))
            .collect()
    }

    #[test]
    fn export_tree_rle_roundtrips() {
        // Uniform histogram: a single run of sixteen 4-bit codes,
        // which needs the escape with the largest repeat count.
        let lengths = vec![4; 16];
        assert_eq!(rle_roundtrip_lengths(16, 8, &lengths), lengths);

        // A single 1-bit code, written as the escape literal; a raw
        // pair of equal lengths and an escaped run of zeroes.
        let lengths = vec![1, 2, 3, 4, 5, 6, 7, 7, 0, 0, 0, 0, 0, 0, 0, 0];
        assert_eq!(rle_roundtrip_lengths(16, 8, &lengths), lengths);

        // A longer escaped run of zeroes, after a raw pair.
        let lengths = vec![1, 2, 3, 4, 5, 5, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        assert_eq!(rle_roundtrip_lengths(16, 8, &lengths), lengths);
    }

    #[test]
    fn export_tree_huffman_roundtrips() {
        // A byte histogram with a wide spread of frequencies, so the
        // tree uses both single literals and RLE runs when exported.
        let mut data = Vec::new();
        for value in 0u32..256 {
            data.extend(std::iter::repeat_n(value as u8, (value % 17) as usize + 1));
        }

        let mut enc = HuffmanEncoder::new(256, 16);
        for &byte in &data {
            enc.histo_one(usize::from(byte));
        }
        enc.compute_tree_from_histo().unwrap();

        let mut out = vec![0u8; 8192];
        let mut bitbuf = BitstreamOut::new(&mut out);
        enc.export_tree_huffman(&mut bitbuf).unwrap();
        for &byte in &data {
            enc.encode_one(&mut bitbuf, usize::from(byte));
        }
        let written = bitbuf.flush();

        let mut dec = HuffmanDecoder::new(256, 16);
        let mut stream = BitstreamIn::new(&out[..written]);
        dec.import_tree_huffman(&mut stream).unwrap();
        for &byte in &data {
            assert_eq!(dec.decode_one(&mut stream), u32::from(byte));
        }
    }

    #[test]
    fn export_tree_huffman_single_symbol() {
        // A tree with one symbol gets a single-bit code; the small
        // tree then has a lone non-zero entry beyond the token.
        let data = [0u8; 300];
        let mut enc = HuffmanEncoder::new(256, 16);
        for &byte in &data {
            enc.histo_one(usize::from(byte));
        }
        enc.compute_tree_from_histo().unwrap();

        let mut out = vec![0u8; 8192];
        let mut bitbuf = BitstreamOut::new(&mut out);
        enc.export_tree_huffman(&mut bitbuf).unwrap();
        for &byte in &data {
            enc.encode_one(&mut bitbuf, usize::from(byte));
        }
        let written = bitbuf.flush();

        let mut dec = HuffmanDecoder::new(256, 16);
        let mut stream = BitstreamIn::new(&out[..written]);
        dec.import_tree_huffman(&mut stream).unwrap();
        for &byte in &data {
            assert_eq!(dec.decode_one(&mut stream), u32::from(byte));
        }
    }
}
