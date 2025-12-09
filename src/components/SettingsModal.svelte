<script>
  import { PlusOutline } from "flowbite-svelte-icons";
  import { Modal, Input, Badge, Label, Select, Toggle, Tooltip, Button, ButtonGroup } from "flowbite-svelte";

  import {
    addToList,
    removeFromList,
    setBool,
    setDirectory,
    setPreferRegions,
    setPreferVersions,
    setSubfolderScheme,
  } from "../mutation.js";
  import { getSettings } from "../query.js";
  import {
    allRegions,
    allRegionsKey,
    allRegionsSubfolders,
    allRegionsSubfoldersKey,
    discardFlags,
    discardFlagsKey,
    discardReleases,
    discardReleasesKey,
    groupSubsystems,
    groupSubsystemsKey,
    isSettingsModalOpen,
    languages,
    languagesKey,
    oneRegions,
    oneRegionsKey,
    oneRegionsSubfolders,
    oneRegionsSubfoldersKey,
    preferFlags,
    preferFlagsKey,
    preferParents,
    preferParentsKey,
    preferRegions,
    preferRegionsChoices,
    preferVersions,
    preferVersionsChoices,
    romDirectory,
    romDirectoryKey,
    strictOneRegions,
    strictOneRegionsKey,
    tmpDirectory,
    tmpDirectoryKey,
    subfolderSchemesChoices,
  } from "../store.js";

  let addOneRegion = "";
  let addAllRegion = "";
  let addLanguage = "";
  let addDiscardRelease = "";
  let addDiscardFlag = "";
  let addPreferFlag = "";

  const onAddToListChange = async (key, value) => {
    if (value) {
      await addToList(key, value);
      await getSettings();
    }
  };

  const onAddOneRegionClick = async () => {
    await onAddToListChange(oneRegionsKey, addOneRegion);
    addOneRegion = "";
  };

  const onAddAllRegionClick = async () => {
    await onAddToListChange(allRegionsKey, addAllRegion);
    addAllRegion = "";
  };

  const onAddLanguageClick = async () => {
    await onAddToListChange(languagesKey, addLanguage);
    addLanguage = "";
  };

  const onAddDiscardReleaseClick = async () => {
    await onAddToListChange(discardReleasesKey, addDiscardRelease);
    addDiscardRelease = "";
  };

  const onAddDiscardFlagClick = async () => {
    await onAddToListChange(discardFlagsKey, addDiscardFlag);
    addDiscardFlag = "";
  };

  const onAddPreferFlagClick = async () => {
    await onAddToListChange(preferFlagsKey, addPreferFlag);
    addPreferFlag = "";
  };

  const onRemoveFromListClick = async (key, value) => {
    if (value) {
      await removeFromList(key, value);
      await getSettings();
    }
  };

  const onSwitchChange = async (key, value) => {
    await setBool(key, value);
    await getSettings();
  };

  const onStrictOneRegionsChange = async () => {
    await onSwitchChange(strictOneRegionsKey, $strictOneRegions);
  };

  const onPreferParentsChange = async () => {
    await onSwitchChange(preferParentsKey, $preferParents);
  };

  const onGroupSubsystemsChange = async () => {
    await onSwitchChange(groupSubsystemsKey, $groupSubsystems);
  };

  const onPreferRegionsChange = async () => {
    await setPreferRegions($preferRegions);
    await getSettings();
  };

  const onPreferVersionsChange = async () => {
    await setPreferVersions($preferVersions);
    await getSettings();
  };

  const onOneRegionsSubfoldersChange = async () => {
    await setSubfolderScheme(oneRegionsSubfoldersKey, $oneRegionsSubfolders);
    await getSettings();
  };

  const onAllRegionsSubfoldersChange = async () => {
    await setSubfolderScheme(allRegionsSubfoldersKey, $allRegionsSubfolders);
    await getSettings();
  };

  const onDirectoryChange = async (key, value) => {
    if (value) {
      await setDirectory(key, value);
      await getSettings();
    }
  };

  const onRomDirectoryChange = async () => {
    await onDirectoryChange(romDirectoryKey, $romDirectory);
  };

  const onTmpDirectoryChange = async () => {
    await onDirectoryChange(tmpDirectoryKey, $tmpDirectory);
  };
</script>

<Modal title="Settings" bind:open={$isSettingsModalOpen} size="md" class="text-start">
  <div class="space-y-4">
    <h6 class="text-sm font-medium text-gray-500 uppercase dark:text-gray-400">REGIONS/LANGUAGES</h6>
    <div class="mb-4">
      <Label for="one-regions" class="mb-2">1G1R Regions</Label>
      <ButtonGroup class="w-full">
        <Input
          id="one-regions"
          placeholder="1G1R Regions"
          bind:value={addOneRegion}
          onkeydown={(e) => e.key === "Enter" && onAddOneRegionClick()}
        />
        <Button size="sm" color="primary" onclick={onAddOneRegionClick}>
          <PlusOutline class="h-4 w-4" />
        </Button>
      </ButtonGroup>
      <Tooltip triggeredBy="#one-regions" placement="left">2 letters, uppercase, ordered</Tooltip>
    </div>
    <div class="mb-4 flex flex-wrap gap-2">
      {#each $oneRegions as oneRegion (oneRegion)}
        <Badge
          dismissable
          large
          class="flex items-center gap-1"
          onclose={() => onRemoveFromListClick(oneRegionsKey, oneRegion)}
        >
          {oneRegion}
        </Badge>
      {/each}
    </div>
    <div class="mb-4">
      <Label for="all-regions" class="mb-2">All Regions</Label>
      <ButtonGroup class="w-full">
        <Input
          id="all-regions"
          placeholder="All Regions"
          bind:value={addAllRegion}
          onkeydown={(e) => e.key === "Enter" && onAddAllRegionClick()}
        />
        <Button size="sm" color="primary" onclick={onAddAllRegionClick}>
          <PlusOutline class="h-4 w-4" />
        </Button>
      </ButtonGroup>
      <Tooltip triggeredBy="#all-regions" placement="left">2 letters, uppercase, unordered</Tooltip>
    </div>
    <div class="mb-4 flex flex-wrap gap-2">
      {#each $allRegions as allRegion (allRegion)}
        <Badge
          dismissable
          large
          class="flex items-center gap-1"
          onclose={() => onRemoveFromListClick(allRegionsKey, allRegion)}
        >
          {allRegion}
        </Badge>
      {/each}
    </div>
    <div class="mb-4">
      <Label for="languages" class="mb-2">Languages</Label>
      <ButtonGroup class="w-full">
        <Input
          id="languages"
          placeholder="Languages"
          bind:value={addLanguage}
          onkeydown={(e) => e.key === "Enter" && onAddLanguageClick()}
        />
        <Button size="sm" color="primary" onclick={onAddLanguageClick}>
          <PlusOutline class="h-4 w-4" />
        </Button>
      </ButtonGroup>
      <Tooltip triggeredBy="#languages" placement="left">2 letters, capitalized</Tooltip>
    </div>
    <div class="mb-4 flex flex-wrap gap-2">
      {#each $languages as language (language)}
        <Badge
          dismissable
          large
          class="flex items-center gap-1"
          onclose={() => onRemoveFromListClick(languagesKey, language)}
        >
          {language}
        </Badge>
      {/each}
    </div>
    <h6 class="text-sm font-medium text-gray-500 uppercase dark:text-gray-400">FILTERS</h6>
    <div class="mb-4">
      <Label for="discard-releases" class="mb-2">Discard Releases</Label>
      <ButtonGroup class="w-full">
        <Input
          id="discard-releases"
          placeholder="Discard Releases"
          bind:value={addDiscardRelease}
          onkeydown={(e) => e.key === "Enter" && onAddDiscardReleaseClick()}
        />
        <Button size="sm" color="primary" onclick={onAddDiscardReleaseClick}>
          <PlusOutline class="h-4 w-4" />
        </Button>
      </ButtonGroup>
      <Tooltip triggeredBy="#discard-releases" placement="left">Discard specific releases</Tooltip>
    </div>
    <div class="mb-4 flex flex-wrap gap-2">
      {#each $discardReleases as discardRelease (discardRelease)}
        <Badge
          dismissable
          large
          class="flex items-center gap-1"
          onclose={() => onRemoveFromListClick(discardReleasesKey, discardRelease)}
        >
          {discardRelease}
        </Badge>
      {/each}
    </div>
    <div class="mb-4">
      <Label for="discard-flags" class="mb-2">Discard Flags</Label>
      <ButtonGroup class="w-full">
        <Input
          id="discard-flags"
          placeholder="Discard Flags"
          bind:value={addDiscardFlag}
          onkeydown={(e) => e.key === "Enter" && onAddDiscardFlagClick()}
        />
        <Button size="sm" color="primary" onclick={onAddDiscardFlagClick}>
          <PlusOutline class="h-4 w-4" />
        </Button>
      </ButtonGroup>
      <Tooltip triggeredBy="#discard-flags" placement="left">Discard specific flags</Tooltip>
    </div>
    <div class="mb-4 flex flex-wrap gap-2">
      {#each $discardFlags as discardFlag (discardFlag)}
        <Badge
          dismissable
          large
          class="flex items-center gap-1"
          onclose={() => onRemoveFromListClick(discardFlagsKey, discardFlag)}
        >
          {discardFlag}
        </Badge>
      {/each}
    </div>
    <h6 class="text-sm font-medium text-gray-500 uppercase dark:text-gray-400">SORTING</h6>
    <div id="strict-one-regions" class="mb-4">
      <Toggle bind:checked={$strictOneRegions} onchange={onStrictOneRegionsChange}>Strict 1G1R</Toggle>
      <Tooltip triggeredBy="#strict-one-regions" placement="left"
        >Strict mode elects games regardless of their completion</Tooltip
      >
    </div>
    <div id="prefer-parents" class="mb-4">
      <Toggle bind:checked={$preferParents} onchange={onPreferParentsChange}>Prefer Parents</Toggle>
      <Tooltip triggeredBy="#prefer-parents" placement="left">Favor parents vs clones in the election process</Tooltip>
    </div>
    <div id="prefer-regions" class="mb-4">
      <Label for="prefer-regions-select" class="mb-2">Prefer Regions</Label>
      <Select id="prefer-regions-select" bind:value={$preferRegions} onchange={onPreferRegionsChange}>
        {#each preferRegionsChoices as preferRegionChoice (preferRegionChoice)}
          <option value={preferRegionChoice}>{preferRegionChoice}</option>
        {/each}
      </Select>
      <Tooltip triggeredBy="#prefer-regions-select" placement="left">
        Broad favors games targeting more regions, narrow favors fewer regions
      </Tooltip>
    </div>
    <div id="prefer-versions" class="mb-4">
      <Label for="prefer-versions-select" class="mb-2">Prefer Versions</Label>
      <Select id="prefer-versions-select" bind:value={$preferVersions} onchange={onPreferVersionsChange}>
        {#each preferVersionsChoices as preferVersionChoice (preferVersionChoice)}
          <option value={preferVersionChoice}>{preferVersionChoice}</option>
        {/each}
      </Select>
      <Tooltip triggeredBy="#prefer-versions-select" placement="left">
        New favors newer revisions, old favors older
      </Tooltip>
    </div>
    <div class="mb-4">
      <Label for="prefer-flags" class="mb-2">Prefer Flags</Label>
      <ButtonGroup class="w-full">
        <Input
          id="prefer-flags"
          placeholder="Prefer Flags"
          bind:value={addPreferFlag}
          onkeydown={(e) => e.key === "Enter" && onAddPreferFlagClick()}
        />
        <Button size="sm" color="primary" onclick={onAddPreferFlagClick}>
          <PlusOutline class="h-4 w-4" />
        </Button>
      </ButtonGroup>
      <Tooltip triggeredBy="#prefer-flags" placement="left">Favors specific flags in the election process</Tooltip>
    </div>
    <div class="mb-4 flex flex-wrap gap-2">
      {#each $preferFlags as preferFlag (preferFlag)}
        <Badge
          dismissable
          large
          class="flex items-center gap-1"
          onclose={() => onRemoveFromListClick(preferFlagsKey, preferFlag)}
        >
          {preferFlag}
        </Badge>
      {/each}
    </div>
    <h6 class="text-sm font-medium text-gray-500 uppercase dark:text-gray-400">DIRECTORIES</h6>
    <div class="mb-4">
      <Label for="rom-directory" class="mb-2">ROM Directory</Label>
      <Input
        id="rom-directory"
        placeholder="ROM Directory"
        bind:value={$romDirectory}
        onchange={onRomDirectoryChange}
      />
      <Tooltip triggeredBy="#rom-directory" placement="left">Root directory where ROMs will be stored</Tooltip>
    </div>
    <div class="mb-4">
      <Label for="tmp-directory" class="mb-2">TMP Directory</Label>
      <Input
        id="tmp-directory"
        placeholder="TMP Directory"
        bind:value={$tmpDirectory}
        onchange={onTmpDirectoryChange}
      />
      <Tooltip triggeredBy="#tmp-directory" placement="left">Temporary directory where ROMs will be extrated</Tooltip>
    </div>
    <div id="group-subsystems" class="mb-4">
      <Toggle bind:checked={$groupSubsystems} onchange={onGroupSubsystemsChange}>Group Subsystems</Toggle>
      <Tooltip triggeredBy="#group-subsystems" placement="left">
        Group subsystems in the main system directory (eg: PS3 DLCs and updates)
      </Tooltip>
    </div>
    <div id="one-regions-subfolders" class="mb-4">
      <Label for="one-regions-subfolders-select" class="mb-2">1G1R Subfolders</Label>
      <Select
        id="one-regions-subfolders-select"
        bind:value={$oneRegionsSubfolders}
        onchange={onOneRegionsSubfoldersChange}
      >
        {#each subfolderSchemesChoices as subfolderSchemeChoice (subfolderSchemeChoice)}
          <option value={subfolderSchemeChoice}>{subfolderSchemeChoice}</option>
        {/each}
      </Select>
      <Tooltip triggeredBy="#one-regions-subfolders-select" placement="left">Store 1G1R games in subfolders</Tooltip>
    </div>
    <div id="all-regions-subfolders" class="mb-4">
      <Label for="all-regions-subfolders-select" class="mb-2">All Subfolders</Label>
      <Select
        id="all-regions-subfolders-select"
        bind:value={$allRegionsSubfolders}
        onchange={onAllRegionsSubfoldersChange}
      >
        {#each subfolderSchemesChoices as subfolderSchemeChoice (subfolderSchemeChoice)}
          <option value={subfolderSchemeChoice}>{subfolderSchemeChoice}</option>
        {/each}
      </Select>
      <Tooltip triggeredBy="#all-regions-subfolders-select" placement="left">Store all games in subfolders</Tooltip>
    </div>
  </div>
</Modal>
