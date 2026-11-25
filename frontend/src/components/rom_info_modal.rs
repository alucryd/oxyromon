//! ROM info modal: what the ROM is, what it weighs, the hashes its DAT
//! declares, the ROM file it lives in and that file's format. Every
//! field is already in the row it was opened from — this modal reads no
//! server; it only says what the list leaves as a colour.

use leptos::prelude::*;

use crate::components::info::{InfoBytes, InfoRow, Mono, romfile_format};
use crate::model::Rom;
use crate::ui::Modal;

/// One hash, or nothing when the DAT does not declare it: some do not,
/// and a missing hash is a fact about the DAT, not an error.
#[component]
fn HashLine(label: &'static str, value: Option<String>) -> impl IntoView {
    value.map(|value| {
        view! {
            <InfoRow label=label>
                <Mono>{value}</Mono>
            </InfoRow>
        }
    })
}

/// The modal itself, driven by `open` and `target`.
#[component]
pub fn RomInfoModal(open: RwSignal<bool>, target: RwSignal<Option<Rom>>) -> impl IntoView {
    view! {
        <Modal
            open=open
            title=Signal::derive(move || {
                target
                    .with(|rom| rom.as_ref().map(|rom| rom.name.clone()).unwrap_or_default())
            })
            size="sm"
        >
            {move || {
                target.get().map(|rom| {
                    let missing = rom.romfile.is_none();
                    let crc = rom.crc.clone();
                    let md5 = rom.md5.clone();
                    let sha1 = rom.sha1.clone();
                    view! {
                        <div class="wa-stack wa-gap-l">
                            <div class="wa-stack wa-gap-xs">
                                <InfoRow label="Size"><InfoBytes value=rom.size/></InfoRow>
                                <HashLine label="CRC" value=crc/>
                                <HashLine label="MD5" value=md5/>
                                <HashLine label="SHA1" value=sha1/>
                            </div>

                            <wa-divider></wa-divider>

                            <Show
                                when=move || !missing
                                fallback=move || {
                                    view! {
                                        <div class="wa-flank wa-gap-s wa-align-items-center">
                                            <wa-icon name="circle-exclamation"></wa-icon>
                                            <span>
                                                "No ROM file holds it: this ROM is still wanted."
                                            </span>
                                        </div>
                                    }
                                }
                            >
                                <div class="wa-stack wa-gap-xs">
                                    <div class="wa-flank wa-gap-s wa-align-items-center">
                                        <wa-icon name="folder-open"></wa-icon>
                                        <span class="wa-font-weight-semibold">"ROM File"</span>
                                    </div>
                                    {rom
                                        .romfile
                                        .clone()
                                        .map(|romfile| {
                                            let format = romfile_format(&romfile.path);
                                            view! {
                                                <div class="wa-stack wa-gap-xs">
                                                    <InfoRow label="Path"><Mono>{romfile.path.clone()}</Mono></InfoRow>
                                                    <InfoRow label="Format">
                                                        <wa-badge appearance="outlined" pill="">
                                                            {format}
                                                        </wa-badge>
                                                    </InfoRow>
                                                    <InfoRow label="File Size">
                                                        <InfoBytes value=romfile.size/>
                                                    </InfoRow>
                                                </div>
                                            }
                                        })}
                                </div>
                            </Show>
                        </div>
                    }
                })
            }}
        </Modal>
    }
}
