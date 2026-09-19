//! `prod.keys` / `title.keys` loading and key derivation.
//!
//! Faithful port of `nsz.nut.Keys`: parse `name = HEX` lines, CRC32-verify the
//! known sources/master keys, and derive the per-revision `title_kek` and
//! `key_area_key[app|ocean|system]` tables via the AES-ECB kek chain.

use std::collections::HashMap;
use std::path::Path;

use crate::crypto::ecb;
use crate::error::{Error, Result};

/// CRC32 checksums of the legitimate key sources / master keys (from
/// `nsz/nut/Keys.py`). Used to sanity-check that a loaded key is the real one.
/// Keys absent from this map are accepted without verification.
const CRC32_CHECKSUMS: &[(&str, u32)] = &[
    ("aes_kek_generation_source", 2545229389),
    ("aes_key_generation_source", 459881589),
    ("titlekek_source", 3510501772),
    ("key_area_key_application_source", 4130296074),
    ("key_area_key_ocean_source", 3975316347),
    ("key_area_key_system_source", 4024798875),
    ("master_key_00", 3540309694),
    ("master_key_01", 3477638116),
    ("master_key_02", 2087460235),
    ("master_key_03", 4095912905),
    ("master_key_04", 3833085536),
    ("master_key_05", 2078263136),
    ("master_key_06", 2812171174),
    ("master_key_07", 1146095808),
    ("master_key_08", 1605958034),
    ("master_key_09", 3456782962),
    ("master_key_0a", 2012895168),
    ("master_key_0b", 3813624150),
    ("master_key_0c", 3881579466),
    ("master_key_0d", 723654444),
    ("master_key_0e", 2690905064),
    ("master_key_0f", 4082108335),
    ("master_key_10", 788455323),
    ("master_key_11", 1214507020),
    ("master_key_12", 1051942134),
    ("master_key_13", 2476807835),
    ("master_key_14", 2448653557),
    ("master_key_15", 4071812001),
];

/// Key area selector matching `key_area_key_{application,ocean,system}_source`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyArea {
    Application = 0,
    Ocean = 1,
    System = 2,
}

/// A loaded, derived key set.
#[derive(Debug, Clone, Default)]
pub struct Keys {
    raw: HashMap<String, Vec<u8>>,
    title_keks: Vec<Option<[u8; 16]>>,
    key_area_keys: Vec<[Option<[u8; 16]>; 3]>,
    /// rightsId (lowercase hex) -> encrypted title key, from `title.keys`.
    title_keys: HashMap<String, [u8; 16]>,
}

/// Read a keys file, naming it in the error: with keys loaded lazily, the
/// failure can surface far from where the path was given.
fn read_text(path: &Path) -> Result<String> {
    std::fs::read_to_string(path)
        .map_err(|e| std::io::Error::new(e.kind(), format!("{}: {e}", path.display())).into())
}

fn parse_line(line: &str) -> Option<(String, String)> {
    // Mirrors: r"\s*([a-z0-9_]+)\s*=\s*([A-F0-9]+)\s*" (case-insensitive)
    let eq = line.find('=')?;
    let key = line[..eq].trim();
    let val = line[eq + 1..].trim();
    if key.is_empty() || val.is_empty() {
        return None;
    }
    let key_ok = key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    let val_ok = val.chars().all(|c| c.is_ascii_hexdigit());
    if key_ok && val_ok {
        Some((key.to_string(), val.to_uppercase()))
    } else {
        None
    }
}

impl Keys {
    /// Load and derive from a `prod.keys` file.
    ///
    /// If `verify_crc` is true, every key present in the checksum table is
    /// CRC32-verified and a mismatch is an error. Synthetic test keys should pass
    /// `false`.
    pub fn load(path: impl AsRef<Path>, verify_crc: bool) -> Result<Self> {
        Self::from_str(&read_text(path.as_ref())?, verify_crc)
    }

    pub fn from_str(text: &str, verify_crc: bool) -> Result<Self> {
        let mut raw = HashMap::new();
        for line in text.lines() {
            if let Some((k, v)) = parse_line(line) {
                let bytes = hex::decode(&v)?;
                raw.insert(k, bytes);
            }
        }

        // Every key-derivation input is one AES block; reject anything else up
        // front so the ECB steps below can't fail.
        let get = |name: &str| -> Result<[u8; 16]> {
            let bytes = raw
                .get(name)
                .cloned()
                .or_else(|| raw.get(&name.replace("nca_", "")).cloned())
                .ok_or_else(|| Error::MissingKey(name.to_string()))?;
            if verify_crc {
                if let Some(&(_, expected)) = CRC32_CHECKSUMS.iter().find(|(k, _)| *k == name) {
                    let mut hasher = crc32fast::Hasher::new();
                    hasher.update(&bytes);
                    let got = hasher.finalize();
                    if got != expected {
                        return Err(Error::InvalidKey(format!(
                            "{name} crc32 mismatch (expected {expected}, got {got})"
                        )));
                    }
                }
            }
            bytes
                .try_into()
                .map_err(|_| Error::InvalidKey(format!("{name} is not 16 bytes")))
        };

        let aes_kek_gen = get("aes_kek_generation_source")?;
        let aes_key_gen = get("aes_key_generation_source")?;
        let titlekek_source = get("titlekek_source")?;
        let kaa = get("key_area_key_application_source")?;
        let kao = get("key_area_key_ocean_source")?;
        let kas = get("key_area_key_system_source")?;

        let mut title_keks = vec![None; 32];
        let mut key_area_keys = vec![[None, None, None]; 32];

        for rev in 0..32 {
            let name = format!("master_key_{rev:02x}");
            let mk = match get(&name) {
                Ok(m) => m,
                Err(Error::MissingKey(_)) => continue,
                Err(e) => return Err(e),
            };
            // titleKek = ECB(master).decrypt(titlekek_source)
            title_keks[rev] = Some(ecb_decrypt(&mk, titlekek_source));
            key_area_keys[rev] =
                [kaa, kao, kas].map(|src| Some(generate_kek(src, &mk, aes_kek_gen, aes_key_gen)));
        }

        Ok(Keys {
            raw,
            title_keks,
            key_area_keys,
            title_keys: HashMap::new(),
        })
    }

    /// Load a `title.keys` file (rightsId = titleKeyHex) into the rights map.
    pub fn load_title_keys(&mut self, path: impl AsRef<Path>) -> Result<()> {
        let text = read_text(path.as_ref())?;
        for line in text.lines() {
            if let Some((k, v)) = parse_line(line) {
                let bytes = hex::decode(&v)?;
                if bytes.len() == 16 {
                    let mut arr = [0u8; 16];
                    arr.copy_from_slice(&bytes);
                    self.title_keys.insert(k.to_lowercase(), arr);
                }
            }
        }
        Ok(())
    }

    pub fn get_raw(&self, name: &str) -> Option<&[u8]> {
        self.raw.get(name).map(|v| v.as_slice())
    }

    pub fn has_master_key(&self, rev: usize) -> bool {
        rev < 32 && self.raw.contains_key(&format!("master_key_{rev:02x}"))
    }

    pub fn title_kek(&self, rev: usize) -> Result<[u8; 16]> {
        self.title_keks
            .get(rev)
            .and_then(|x| *x)
            .ok_or_else(|| Error::MissingKey(format!("title_kek[{rev}]")))
    }

    pub fn key_area_key(&self, rev: usize, area: KeyArea) -> Result<[u8; 16]> {
        self.key_area_keys
            .get(rev)
            .and_then(|a| a[area as usize])
            .ok_or_else(|| Error::MissingKey(format!("key_area_key[{rev}][{area:?}]")))
    }

    /// Decrypt a title key wrapped with the titlekek of revision `rev`
    /// (`Keys.decryptTitleKey`).
    pub fn decrypt_title_key(&self, wrapped: &[u8; 16], rev: usize) -> Result<[u8; 16]> {
        Ok(ecb_decrypt(&self.title_kek(rev)?, *wrapped))
    }

    /// Unwrap an AES-wrapped titlekey from an NCA header keyblock
    /// (`Keys.unwrapAesWrappedTitlekey`). Always uses the application key area,
    /// matching the Python implementation.
    pub fn unwrap_title_key(&self, wrapped: &[u8; 16], key_generation: usize) -> Result<[u8; 16]> {
        let kek = self.key_area_key(key_generation, KeyArea::Application)?;
        Ok(ecb_decrypt(&kek, *wrapped))
    }

    /// Inverse of [`Self::unwrap_title_key`]: wrap a plaintext key so that
    /// `unwrap_title_key(wrapped, key_generation)` returns `plain`. Used to build
    /// synthetic NCA keyblocks in tests.
    pub fn wrap_title_key(&self, plain: &[u8; 16], key_generation: usize) -> Result<[u8; 16]> {
        let mut out = *plain;
        ecb::encrypt_block(
            &self.key_area_key(key_generation, KeyArea::Application)?,
            &mut out,
        );
        Ok(out)
    }

    /// The 32-byte XTS key protecting the first 0xC00 bytes of every NCA.
    pub fn header_key(&self) -> Result<[u8; 32]> {
        self.get_raw("header_key")
            .ok_or_else(|| Error::MissingKey("header_key".into()))?
            .try_into()
            .map_err(|_| Error::InvalidKey("header_key must be 32 bytes".into()))
    }

    /// Look up an encrypted title key by rightsId (lowercase hex).
    pub fn title_key_by_rights(&self, rights_id: &str) -> Option<[u8; 16]> {
        self.title_keys.get(&rights_id.to_lowercase()).copied()
    }
}

/// `Keys.generateKek`: kek = ECB(master).decrypt(kek_seed);
/// src_kek = ECB(kek).decrypt(src); key = ECB(src_kek).decrypt(key_seed).
fn generate_kek(
    src: [u8; 16],
    master: &[u8; 16],
    kek_seed: [u8; 16],
    key_seed: [u8; 16],
) -> [u8; 16] {
    let kek = ecb_decrypt(master, kek_seed);
    let src_kek = ecb_decrypt(&kek, src);
    ecb_decrypt(&src_kek, key_seed)
}

fn ecb_decrypt(key: &[u8; 16], mut block: [u8; 16]) -> [u8; 16] {
    ecb::decrypt_block(key, &mut block);
    block
}
