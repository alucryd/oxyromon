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
use crate::ui::Modal;

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
            <div class="grid grid-cols-2 gap-x-8 text-start">
                // Left column: directories + regions/languages.
                <div class="space-y-4">
                    <h6 class="text-sm font-medium text-gray-500 uppercase dark:text-gray-400">
                        DIRECTORIES
                    </h6>
                    <DirectoryField
                        label="ROM Directory"
                        value=local.rom_directory
                        on_change=Callback::new(move |v| change_directory(ROM_DIRECTORY_KEY, v))
                    />
                    <DirectoryField
                        label="TMP Directory"
                        value=local.tmp_directory
                        on_change=Callback::new(move |v| change_directory(TMP_DIRECTORY_KEY, v))
                    />
                    <ToggleField
                        label="Group Subsystems"
                        value=local.group_subsystems
                        on_toggle=Callback::new(move |v| toggle(GROUP_SUBSYSTEMS_KEY, v))
                    />
                    <SelectField
                        label="1G1R Subfolders"
                        value=local.one_regions_subfolders
                        choices=&SUBFOLDER_SCHEMES_CHOICES
                        on_select=Callback::new(move |v| {
                            choose_subfolder(ONE_REGIONS_SUBFOLDERS_KEY, v)
                        })
                    />
                    <SelectField
                        label="All Subfolders"
                        value=local.all_regions_subfolders
                        choices=&SUBFOLDER_SCHEMES_CHOICES
                        on_select=Callback::new(move |v| {
                            choose_subfolder(ALL_REGIONS_SUBFOLDERS_KEY, v)
                        })
                    />

                    <h6 class="text-sm font-medium text-gray-500 uppercase dark:text-gray-400">
                        REGIONS/LANGUAGES
                    </h6>
                    <ListField
                        label="1G1R Regions"
                        placeholder="1G1R Regions"
                        setting_key=ONE_REGIONS_KEY
                        items=local.one_regions
                        system_id=system_id
                        reload=reload
                    />
                    <ListField
                        label="All Regions"
                        placeholder="All Regions"
                        setting_key=ALL_REGIONS_KEY
                        items=local.all_regions
                        system_id=system_id
                        reload=reload
                    />
                    <ListField
                        label="Languages"
                        placeholder="Languages"
                        setting_key=LANGUAGES_KEY
                        items=local.languages
                        system_id=system_id
                        reload=reload
                    />
                </div>

                // Right column: sorting + filters.
                <div class="space-y-4">
                    <h6 class="text-sm font-medium text-gray-500 uppercase dark:text-gray-400">
                        SORTING
                    </h6>
                    <ToggleField
                        label="Strict 1G1R"
                        value=local.strict_one_regions
                        on_toggle=Callback::new(move |v| toggle(STRICT_ONE_REGIONS_KEY, v))
                    />
                    <ToggleField
                        label="Prefer Parents"
                        value=local.prefer_parents
                        on_toggle=Callback::new(move |v| toggle(PREFER_PARENTS_KEY, v))
                    />
                    <SelectField
                        label="Prefer Regions"
                        value=local.prefer_regions
                        choices=&PREFER_REGIONS_CHOICES
                        on_select=Callback::new(choose_prefer_regions)
                    />
                    <SelectField
                        label="Prefer Versions"
                        value=local.prefer_versions
                        choices=&PREFER_VERSIONS_CHOICES
                        on_select=Callback::new(choose_prefer_versions)
                    />
                    <ListField
                        label="Prefer Flags"
                        placeholder="Prefer Flags"
                        setting_key=PREFER_FLAGS_KEY
                        items=local.prefer_flags
                        system_id=system_id
                        reload=reload
                    />

                    <h6 class="text-sm font-medium text-gray-500 uppercase dark:text-gray-400">
                        FILTERS
                    </h6>
                    <ListField
                        label="Discard Releases"
                        placeholder="Discard Releases"
                        setting_key=DISCARD_RELEASES_KEY
                        items=local.discard_releases
                        system_id=system_id
                        reload=reload
                    />
                    <ListField
                        label="Discard Flags"
                        placeholder="Discard Flags"
                        setting_key=DISCARD_FLAGS_KEY
                        items=local.discard_flags
                        system_id=system_id
                        reload=reload
                    />
                </div>
            </div>
        </Modal>
    }
}

const LABEL: &str = "mb-2 block text-sm font-medium text-gray-900 dark:text-gray-300";
const INPUT: &str = "block w-full rounded-lg border border-gray-300 bg-gray-50 p-2.5 text-gray-900 dark:border-gray-600 dark:bg-gray-700 dark:text-white";

#[component]
fn DirectoryField(
    #[prop(into)] label: String,
    value: RwSignal<String>,
    on_change: Callback<String>,
) -> impl IntoView {
    view! {
        <div class="mb-4">
            <label class=LABEL>{label}</label>
            <input
                class=INPUT
                prop:value=move || value.get()
                on:input=move |ev| value.set(event_target_value(&ev))
                on:change=move |ev| on_change.run(event_target_value(&ev))
            />
        </div>
    }
}

#[component]
fn ToggleField(
    #[prop(into)] label: String,
    value: RwSignal<bool>,
    on_toggle: Callback<bool>,
) -> impl IntoView {
    view! {
        <div class="mb-4">
            <label class="inline-flex cursor-pointer items-center gap-2">
                <input
                    type="checkbox"
                    class="h-4 w-4"
                    prop:checked=move || value.get()
                    on:change=move |_| {
                        let next = !value.get_untracked();
                        value.set(next);
                        on_toggle.run(next);
                    }
                />
                <span>{label}</span>
            </label>
        </div>
    }
}

#[component]
fn SelectField(
    #[prop(into)] label: String,
    value: RwSignal<String>,
    choices: &'static [&'static str],
    on_select: Callback<String>,
) -> impl IntoView {
    view! {
        <div class="mb-4">
            <label class=LABEL>{label}</label>
            <select
                class=INPUT
                prop:value=move || value.get()
                on:change=move |ev| {
                    let v = event_target_value(&ev);
                    value.set(v.clone());
                    on_select.run(v);
                }
            >
                {choices
                    .iter()
                    .map(|choice| {
                        view! {
                            <option value=*choice selected=move || value.get() == *choice>
                                {*choice}
                            </option>
                        }
                    })
                    .collect_view()}
            </select>
        </div>
    }
}

#[component]
fn ListField(
    #[prop(into)] label: String,
    #[prop(into)] placeholder: String,
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
        <div class="mb-4">
            <label class=LABEL>{label}</label>
            <div class="flex w-full">
                <input
                    class="block w-full rounded-l-lg border border-gray-300 bg-gray-50 p-2.5 text-gray-900 dark:border-gray-600 dark:bg-gray-700 dark:text-white"
                    placeholder=placeholder
                    prop:value=move || draft.get()
                    on:input=move |ev| draft.set(event_target_value(&ev))
                    on:keydown=move |ev| {
                        if ev.key() == "Enter" {
                            add();
                        }
                    }
                />
                <button
                    class="flex items-center rounded-r-lg bg-primary-600 px-3 text-white hover:bg-primary-700"
                    on:click=move |_| add()
                >
                    <wa-icon name="plus" label="Add"></wa-icon>
                </button>
            </div>
            <div class="mt-2 flex flex-wrap gap-2">
                <For each=move || items.get() key=|item| item.clone() let:item>
                    {
                        let item_for_remove = item.clone();
                        view! {
                            <span class="inline-flex items-center gap-1 rounded-lg bg-primary-100 px-2.5 py-1 text-sm font-medium text-primary-800 dark:bg-primary-900 dark:text-primary-300">
                                {item.clone()}
                                <button
                                    class="ml-1 text-primary-600 hover:text-primary-900 dark:text-primary-300"
                                    on:click=move |_| remove(item_for_remove.clone())
                                >
                                    "×"
                                </button>
                            </span>
                        }
                    }
                </For>
            </div>
        </div>
    }
}
