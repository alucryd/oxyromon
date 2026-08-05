//! Settings dialog (global or per-system). Ports `SettingsModal.svelte`.
//!
//! Each control writes its change through a GraphQL mutation and then reloads
//! the settings for the current `system_id`, mirroring the original component's
//! `reload()` behaviour.

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::{
    add_to_list, get_raw_settings, remove_from_list, report_error, set_bool, set_directory,
    set_prefer_regions, set_prefer_versions, set_subfolder_scheme,
};
use crate::model::Setting;
use crate::state::{
    ALL_REGIONS_KEY, ALL_REGIONS_SUBFOLDERS_KEY, AppState, DISCARD_FLAGS_KEY, DISCARD_RELEASES_KEY,
    GROUP_SUBSYSTEMS_KEY, LANGUAGES_KEY, ONE_REGIONS_KEY, ONE_REGIONS_SUBFOLDERS_KEY,
    PREFER_FLAGS_KEY, PREFER_PARENTS_KEY, PREFER_REGIONS_CHOICES, PREFER_REGIONS_KEY,
    PREFER_VERSIONS_CHOICES, PREFER_VERSIONS_KEY, ROM_DIRECTORY_KEY, STRICT_ONE_REGIONS_KEY,
    SUBFOLDER_SCHEMES_CHOICES, TMP_DIRECTORY_KEY, split_list,
};
use crate::ui::{Modal, control_checked, control_value};

/// All the local, editable copies of the settings shown in the modal.
#[derive(Clone, Copy)]
struct Local {
    one_regions: RwSignal<Vec<String>>,
    all_regions: RwSignal<Vec<String>>,
    languages: RwSignal<Vec<String>>,
    discard_releases: RwSignal<Vec<String>>,
    discard_flags: RwSignal<Vec<String>>,
    prefer_flags: RwSignal<Vec<String>>,
    strict_one_regions: RwSignal<bool>,
    prefer_parents: RwSignal<bool>,
    group_subsystems: RwSignal<bool>,
    prefer_regions: RwSignal<String>,
    prefer_versions: RwSignal<String>,
    one_regions_subfolders: RwSignal<String>,
    all_regions_subfolders: RwSignal<String>,
    rom_directory: RwSignal<String>,
    tmp_directory: RwSignal<String>,
}

impl Local {
    fn new() -> Self {
        Self {
            one_regions: RwSignal::new(Vec::new()),
            all_regions: RwSignal::new(Vec::new()),
            languages: RwSignal::new(Vec::new()),
            discard_releases: RwSignal::new(Vec::new()),
            discard_flags: RwSignal::new(Vec::new()),
            prefer_flags: RwSignal::new(Vec::new()),
            strict_one_regions: RwSignal::new(false),
            prefer_parents: RwSignal::new(true),
            group_subsystems: RwSignal::new(true),
            prefer_regions: RwSignal::new("none".to_string()),
            prefer_versions: RwSignal::new("none".to_string()),
            one_regions_subfolders: RwSignal::new("none".to_string()),
            all_regions_subfolders: RwSignal::new("none".to_string()),
            rom_directory: RwSignal::new(String::new()),
            tmp_directory: RwSignal::new(String::new()),
        }
    }

    fn populate(&self, settings: &[Setting]) {
        let find = |key: &str| {
            settings
                .iter()
                .find(|s| s.key == key)
                .and_then(|s| s.value.clone())
        };
        let is_true = |key: &str| find(key).as_deref() == Some("true");
        let not_false = |key: &str| find(key).as_deref() != Some("false");

        self.one_regions.set(split_list(&find(ONE_REGIONS_KEY)));
        self.all_regions.set(split_list(&find(ALL_REGIONS_KEY)));
        self.languages.set(split_list(&find(LANGUAGES_KEY)));
        self.discard_releases
            .set(split_list(&find(DISCARD_RELEASES_KEY)));
        self.discard_flags.set(split_list(&find(DISCARD_FLAGS_KEY)));
        self.prefer_flags.set(split_list(&find(PREFER_FLAGS_KEY)));
        self.strict_one_regions.set(is_true(STRICT_ONE_REGIONS_KEY));
        self.prefer_parents.set(not_false(PREFER_PARENTS_KEY));
        self.group_subsystems.set(not_false(GROUP_SUBSYSTEMS_KEY));
        self.prefer_regions
            .set(find(PREFER_REGIONS_KEY).unwrap_or_else(|| "none".to_string()));
        self.prefer_versions
            .set(find(PREFER_VERSIONS_KEY).unwrap_or_else(|| "none".to_string()));
        self.one_regions_subfolders
            .set(find(ONE_REGIONS_SUBFOLDERS_KEY).unwrap_or_else(|| "none".to_string()));
        self.all_regions_subfolders
            .set(find(ALL_REGIONS_SUBFOLDERS_KEY).unwrap_or_else(|| "none".to_string()));
        self.rom_directory
            .set(find(ROM_DIRECTORY_KEY).unwrap_or_default());
        self.tmp_directory
            .set(find(TMP_DIRECTORY_KEY).unwrap_or_default());
    }
}

#[component]
pub fn SettingsModal(
    open: RwSignal<bool>,
    /// `None` for the global settings, `Some(id)` for a specific system.
    system_id: RwSignal<Option<i64>>,
    title: RwSignal<String>,
) -> impl IntoView {
    let state = expect_context::<AppState>();
    let local = Local::new();

    // (Re)load settings for the active system into the local copies.
    let reload = Callback::new(move |_: ()| {
        let sid = system_id.get_untracked();
        spawn_local(async move {
            match get_raw_settings(sid).await {
                Ok(settings) => local.populate(&settings),
                Err(e) => report_error(state.notifier, "Loading settings", &e),
            }
        });
    });

    // Reload whenever the modal is opened (matches `$: if (open) loadSettings()`).
    Effect::new(move |_| {
        if open.get() {
            reload.run(());
        }
    });

    let toggle = move |key: &'static str, value: bool| {
        let sid = system_id.get_untracked();
        spawn_local(async move {
            if let Err(e) = set_bool(key, value, sid).await {
                report_error(state.notifier, "Updating settings", &e);
            }
            reload.run(());
        });
    };

    let choose_prefer_regions = move |value: String| {
        let sid = system_id.get_untracked();
        spawn_local(async move {
            if let Err(e) = set_prefer_regions(&value, sid).await {
                report_error(state.notifier, "Updating settings", &e);
            }
            reload.run(());
        });
    };
    let choose_prefer_versions = move |value: String| {
        let sid = system_id.get_untracked();
        spawn_local(async move {
            if let Err(e) = set_prefer_versions(&value, sid).await {
                report_error(state.notifier, "Updating settings", &e);
            }
            reload.run(());
        });
    };
    let choose_subfolder = move |key: &'static str, value: String| {
        let sid = system_id.get_untracked();
        spawn_local(async move {
            if let Err(e) = set_subfolder_scheme(key, &value, sid).await {
                report_error(state.notifier, "Updating settings", &e);
            }
            reload.run(());
        });
    };
    let change_directory = move |key: &'static str, value: String| {
        if value.is_empty() {
            return;
        }
        let sid = system_id.get_untracked();
        spawn_local(async move {
            if let Err(e) = set_directory(key, &value, sid).await {
                report_error(state.notifier, "Updating settings", &e);
            }
            reload.run(());
        });
    };

    view! {
        <Modal open=open title=title size="xl">
            // Two columns where there is room, one when there is not.
            <div class="wa-grid wa-gap-xl" style="--min-column-size: 18rem;">
                <div class="wa-stack wa-gap-l">
                    <SettingsSection title="Directories">
                        <DirectoryField
                            label="ROM Directory"
                            hint="Root directory where ROMs will be stored"
                            value=local.rom_directory
                            on_change=Callback::new(move |v| change_directory(ROM_DIRECTORY_KEY, v))
                        />
                        <DirectoryField
                            label="TMP Directory"
                            hint="Temporary directory where ROMs will be extracted"
                            value=local.tmp_directory
                            on_change=Callback::new(move |v| change_directory(TMP_DIRECTORY_KEY, v))
                        />
                        <ToggleField
                            label="Group Subsystems"
                            hint="Group subsystems in the main system directory (eg: PS3 DLCs and updates)"
                            value=local.group_subsystems
                            on_toggle=Callback::new(move |v| toggle(GROUP_SUBSYSTEMS_KEY, v))
                        />
                        <SelectField
                            label="1G1R Subfolders"
                            hint="Store 1G1R games in subfolders"
                            value=local.one_regions_subfolders
                            choices=&SUBFOLDER_SCHEMES_CHOICES
                            on_select=Callback::new(move |v| {
                                choose_subfolder(ONE_REGIONS_SUBFOLDERS_KEY, v)
                            })
                        />
                        <SelectField
                            label="All Subfolders"
                            hint="Store all games in subfolders"
                            value=local.all_regions_subfolders
                            choices=&SUBFOLDER_SCHEMES_CHOICES
                            on_select=Callback::new(move |v| {
                                choose_subfolder(ALL_REGIONS_SUBFOLDERS_KEY, v)
                            })
                        />
                    </SettingsSection>

                    <SettingsSection title="Regions and languages">
                        <ListField
                            label="1G1R Regions"
                            hint="2 letters, uppercase, ordered"
                            setting_key=ONE_REGIONS_KEY
                            items=local.one_regions
                            system_id=system_id
                            reload=reload
                        />
                        <ListField
                            label="All Regions"
                            hint="2 letters, uppercase, unordered"
                            setting_key=ALL_REGIONS_KEY
                            items=local.all_regions
                            system_id=system_id
                            reload=reload
                        />
                        <ListField
                            label="Languages"
                            hint="2 letters, capitalized"
                            setting_key=LANGUAGES_KEY
                            items=local.languages
                            system_id=system_id
                            reload=reload
                        />
                    </SettingsSection>
                </div>

                <div class="wa-stack wa-gap-l">
                    <SettingsSection title="Sorting">
                        <ToggleField
                            label="Strict 1G1R"
                            hint="Strict mode elects games regardless of their completion"
                            value=local.strict_one_regions
                            on_toggle=Callback::new(move |v| toggle(STRICT_ONE_REGIONS_KEY, v))
                        />
                        <ToggleField
                            label="Prefer Parents"
                            hint="Favor parents vs clones in the election process"
                            value=local.prefer_parents
                            on_toggle=Callback::new(move |v| toggle(PREFER_PARENTS_KEY, v))
                        />
                        <SelectField
                            label="Prefer Regions"
                            hint="Broad favors games targeting more regions, narrow favors fewer"
                            value=local.prefer_regions
                            choices=&PREFER_REGIONS_CHOICES
                            on_select=Callback::new(choose_prefer_regions)
                        />
                        <SelectField
                            label="Prefer Versions"
                            hint="New favors newer revisions, old favors older"
                            value=local.prefer_versions
                            choices=&PREFER_VERSIONS_CHOICES
                            on_select=Callback::new(choose_prefer_versions)
                        />
                        <ListField
                            label="Prefer Flags"
                            hint="Favors specific flags in the election process"
                            setting_key=PREFER_FLAGS_KEY
                            items=local.prefer_flags
                            system_id=system_id
                            reload=reload
                        />
                    </SettingsSection>

                    <SettingsSection title="Filters">
                        <ListField
                            label="Discard Releases"
                            hint="Discard specific releases"
                            setting_key=DISCARD_RELEASES_KEY
                            items=local.discard_releases
                            system_id=system_id
                            reload=reload
                        />
                        <ListField
                            label="Discard Flags"
                            hint="Discard specific flags"
                            setting_key=DISCARD_FLAGS_KEY
                            items=local.discard_flags
                            system_id=system_id
                            reload=reload
                        />
                    </SettingsSection>
                </div>
            </div>
        </Modal>
    }
}

/// A titled group of settings.
#[component]
fn SettingsSection(title: &'static str, children: Children) -> impl IntoView {
    view! {
        <div class="wa-stack wa-gap-m">
            <div class="wa-stack wa-gap-3xs">
                <strong>{title}</strong>
                <wa-divider style="--spacing: 0;"></wa-divider>
            </div>
            {children()}
        </div>
    }
}

#[component]
fn DirectoryField(
    #[prop(into)] label: String,
    #[prop(into)] hint: String,
    value: RwSignal<String>,
    on_change: Callback<String>,
) -> impl IntoView {
    view! {
        <wa-input
            label=label
            hint=hint
            prop:value=move || value.get()
            on:change=move |ev| {
                let entered = control_value(&ev);
                value.set(entered.clone());
                on_change.run(entered);
            }
        ></wa-input>
    }
}

#[component]
fn ToggleField(
    #[prop(into)] label: String,
    #[prop(into)] hint: String,
    value: RwSignal<bool>,
    on_toggle: Callback<bool>,
) -> impl IntoView {
    view! {
        <wa-switch
            hint=hint
            prop:checked=move || value.get()
            on:change=move |ev| {
                let checked = control_checked(&ev);
                value.set(checked);
                on_toggle.run(checked);
            }
        >
            {label}
        </wa-switch>
    }
}

#[component]
fn SelectField(
    #[prop(into)] label: String,
    #[prop(into)] hint: String,
    value: RwSignal<String>,
    choices: &'static [&'static str],
    on_select: Callback<String>,
) -> impl IntoView {
    view! {
        <wa-select
            label=label
            hint=hint
            prop:value=move || value.get()
            on:change=move |ev| {
                let chosen = control_value(&ev);
                value.set(chosen.clone());
                on_select.run(chosen);
            }
        >
            {choices
                .iter()
                .map(|choice| view! { <wa-option value=*choice>{*choice}</wa-option> })
                .collect_view()}
        </wa-select>
    }
}

/// A free-form list of short values, entered one at a time and shown as tags.
#[component]
fn ListField(
    #[prop(into)] label: String,
    #[prop(into)] hint: String,
    setting_key: &'static str,
    items: RwSignal<Vec<String>>,
    system_id: RwSignal<Option<i64>>,
    reload: Callback<()>,
) -> impl IntoView {
    let state = expect_context::<AppState>();
    let draft = RwSignal::new(String::new());

    let add = move || {
        let value = draft.get_untracked();
        if value.is_empty() {
            return;
        }
        draft.set(String::new());
        let sid = system_id.get_untracked();
        spawn_local(async move {
            if let Err(e) = add_to_list(setting_key, &value, sid).await {
                report_error(state.notifier, "Updating settings", &e);
            }
            reload.run(());
        });
    };

    let remove = move |value: String| {
        let sid = system_id.get_untracked();
        spawn_local(async move {
            if let Err(e) = remove_from_list(setting_key, &value, sid).await {
                report_error(state.notifier, "Updating settings", &e);
            }
            reload.run(());
        });
    };

    view! {
        <div class="wa-stack wa-gap-2xs">
            <wa-input
                label=label
                hint=hint
                prop:value=move || draft.get()
                on:input=move |ev| draft.set(control_value(&ev))
                on:keydown=move |ev: web_sys::KeyboardEvent| {
                    if ev.key() == "Enter" {
                        add();
                    }
                }
            >
                // In the input's own end slot, so it lines up with the field
                // rather than with the hint underneath it.
                <wa-button slot="end" appearance="plain" size="small" on:click=move |_| add()>
                    <wa-icon name="plus" label="Add"></wa-icon>
                </wa-button>
            </wa-input>
            <Show when=move || !items.get().is_empty()>
                <div class="wa-cluster wa-gap-2xs">
                    <For each=move || items.get() key=|item| item.clone() let:item>
                        {
                            let value = item.clone();
                            view! {
                                <wa-tag
                                    with-remove=""
                                    size="small"
                                    on:wa-remove=move |_: web_sys::Event| remove(value.clone())
                                >
                                    {item.clone()}
                                </wa-tag>
                            }
                        }
                    </For>
                </div>
            </Show>
        </div>
    }
}
