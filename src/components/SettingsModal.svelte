<script>
  import { faTrashCan } from "@fortawesome/free-solid-svg-icons";
  import Fa from "svelte-fa";

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

  export let toggle = undefined;

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

  const onAddOneRegionChange = async () => {
    await onAddToListChange(oneRegionsKey, addOneRegion);
    addOneRegion = "";
  };

  const onAddAllRegionChange = async () => {
    await onAddToListChange(allRegionsKey, addAllRegion);
    addAllRegion = "";
  };

  const onAddLanguageChange = async () => {
    await onAddToListChange(languagesKey, addLanguage);
    addLanguage = "";
  };

  const onAddDiscardReleaseChange = async () => {
    await onAddToListChange(discardReleasesKey, addDiscardRelease);
    addDiscardRelease = "";
  };

  const onAddDiscardFlagChange = async () => {
    await onAddToListChange(discardFlagsKey, addDiscardFlag);
    addDiscardFlag = "";
  };

  const onAddPreferFlagChange = async () => {
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
    await new Promise(setTimeout);
    await setBool(key, value);
    await getSettings();
  };

  const onStrictOneRegionsChange = async () => {
    await onSwitchChange(strictOneRegionsKey, !$strictOneRegions);
  };

  const onPreferParentsChange = async () => {
    await onSwitchChange(preferParentsKey, !$preferParents);
  };

  const onGroupSubsystemsChange = async () => {
    await onSwitchChange(groupSubsystemsKey, !$groupSubsystems);
  };

  const onPreferRegionsChange = async () => {
    await new Promise(setTimeout);
    await setPreferRegions($preferRegions);
    await getSettings();
  };

  const onPreferVersionsChange = async () => {
    await new Promise(setTimeout);
    await setPreferVersions($preferVersions);
    await getSettings();
  };

  const onOneRegionsSubfoldersChange = async () => {
    await new Promise(setTimeout);
    await setSubfolderScheme(oneRegionsSubfoldersKey, $oneRegionsSubfolders);
    await getSettings();
  };

  const onAllRegionsSubfoldersChange = async () => {
    await new Promise(setTimeout);
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

<!-- <Modal isOpen={$isSettingsModalOpen} {toggle} size="xl" class="text-start">
  <ModalHeader {toggle}>Settings</ModalHeader>
  <ModalBody class="pb-0">
    <h6 class="text-muted">REGIONS/LANGUAGES</h6>
    <Row class="mb-2">
      <Col>
        {#each $oneRegions as oneRegion}
          <Badge class="m-1">
            {oneRegion}
            <Button
              small
              class="px-1 py-0 text-center"
              on:click={() => onRemoveFromListClick(oneRegionsKey, oneRegion)}
            >
              <Fa icon={faTrashCan} />
            </Button>
          </Badge>
        {/each}
      </Col>
    </Row>
    <FormGroup floating label="1G1R Regions">
      <Input
        name="one-regions"
        id="one-regions"
        placeholder="1G1R Regions"
        bind:value={addOneRegion}
        on:change={onAddOneRegionChange}
      />
      <Tooltip target="one-regions" placement="left">2 letters, uppercase, ordered</Tooltip>
    </FormGroup>
    <Row class="mb-2">
      <Col>
        {#each $allRegions as allRegion}
          <Badge class="m-1">
            {allRegion}
            <Button
              small
              class="px-1 py-0 text-center"
              on:click={() => onRemoveFromListClick(allRegionsKey, allRegion)}
            >
              <Fa icon={faTrashCan} />
            </Button>
          </Badge>
        {/each}
      </Col>
    </Row>
    <FormGroup floating label="All Regions">
      <Input
        name="all-regions"
        id="all-regions"
        placeholder="All Regions"
        bind:value={addAllRegion}
        on:change={onAddAllRegionChange}
      />
      <Tooltip target="all-regions" placement="left">2 letters, uppercase, unordered</Tooltip>
    </FormGroup>
    <Row class="mb-2">
      <Col>
        {#each $languages as language}
          <Badge class="m-1">
            {language}
            <Button small class="px-1 py-0 text-center" on:click={() => onRemoveFromListClick(languagesKey, language)}>
              <Fa icon={faTrashCan} />
            </Button>
          </Badge>
        {/each}
      </Col>
    </Row>
    <FormGroup floating label="Languages">
      <Input
        name="languages"
        id="languages"
        placeholder="Languages"
        bind:value={addLanguage}
        on:change={onAddLanguageChange}
      />
      <Tooltip target="languages" placement="left">2 letters, capitalized</Tooltip>
    </FormGroup>
    <h6 class="text-muted">FILTERS</h6>
    <Row class="mb-2">
      <Col>
        {#each $discardReleases as discardRelease}
          <Badge class="m-1">
            {discardRelease}
            <Button
              small
              class="px-1 py-0 text-center"
              on:click={() => onRemoveFromListClick(discardReleasesKey, discardRelease)}
            >
              <Fa icon={faTrashCan} />
            </Button>
          </Badge>
        {/each}
      </Col>
    </Row>
    <FormGroup floating label="Discard Releases">
      <Input
        name="discard-releases"
        id="discard-releases"
        placeholder="Discard Releases"
        bind:value={addDiscardRelease}
        on:change={onAddDiscardReleaseChange}
      />
      <Tooltip target="discard-releases" placement="left">Discard specific releases</Tooltip>
    </FormGroup>
    <Row class="mb-2">
      <Col>
        {#each $discardFlags as discardFlag}
          <Badge class="m-1">
            {discardFlag}
            <Button
              small
              class="px-1 py-0 text-center"
              on:click={() => onRemoveFromListClick(discardFlagsKey, discardFlag)}
            >
              <Fa icon={faTrashCan} />
            </Button>
          </Badge>
        {/each}
      </Col>
    </Row>
    <FormGroup floating label="Discard Flags">
      <Input
        name="discard-flags"
        id="discard-flags"
        placeholder="Discard Flags"
        bind:value={addDiscardFlag}
        on:change={onAddDiscardFlagChange}
      />
      <Tooltip target="discard-flags" placement="left">Discard specific flags</Tooltip>
    </FormGroup>
    <h6 class="text-muted">SORTING</h6>
    <FormGroup id="strict-one-regions">
      <Input type="switch" label="Strict 1G1R" bind:checked={$strictOneRegions} on:change={onStrictOneRegionsChange} />
      <Tooltip target="strict-one-regions" placement="left"
        >Strict mode elects games regardless of their completion</Tooltip
      >
    </FormGroup>
    <FormGroup id="prefer-parents">
      <Input type="switch" label="Prefer Parents" bind:checked={$preferParents} on:change={onPreferParentsChange} />
      <Tooltip target="prefer-parents" placement="left">Favor parents vs clones in the election process</Tooltip>
    </FormGroup>
    <FormGroup id="prefer-regions">
      <InputGroup>
        <InputGroupText class="w-25">Prefer Regions</InputGroupText>
        <Input type="select" label="Prefer Regions" bind:value={$preferRegions} on:change={onPreferRegionsChange}>
          {#each preferRegionsChoices as preferRegionChoice}
            <option value={preferRegionChoice}>{preferRegionChoice}</option>
          {/each}
        </Input>
      </InputGroup>
      <Tooltip target="prefer-regions" placement="left"
        >Broad favors games targeting more regions, narrow favors fewer regions</Tooltip
      >
    </FormGroup>
    <FormGroup id="prefer-versions">
      <InputGroup>
        <InputGroupText class="w-25">Prefer Versions</InputGroupText>
        <Input type="select" label="Prefer Versions" bind:value={$preferVersions} on:change={onPreferVersionsChange}>
          {#each preferVersionsChoices as preferVersionChoice}
            <option value={preferVersionChoice}>{preferVersionChoice}</option>
          {/each}
        </Input>
      </InputGroup>
      <Tooltip target="prefer-versions" placement="left">New favors newer revisions, old favors older</Tooltip>
    </FormGroup>
    <Row class="mb-2">
      <Col>
        {#each $preferFlags as preferFlag}
          <Badge class="m-1">
            {preferFlag}
            <Button
              small
              class="px-1 py-0 text-center"
              on:click={() => onRemoveFromListClick(preferFlagsKey, preferFlag)}
            >
              <Fa icon={faTrashCan} />
            </Button>
          </Badge>
        {/each}
      </Col>
    </Row>
    <FormGroup floating label="Prefer Flags">
      <Input
        name="prefer-flags"
        id="prefer-flags"
        placeholder="Prefer Flags"
        bind:value={addPreferFlag}
        on:change={onAddPreferFlagChange}
      />
      <Tooltip target="prefer-flags" placement="left">Favors specific flags in the election process</Tooltip>
    </FormGroup>
    <h6 class="text-muted">DIRECTORIES</h6>
    <FormGroup floating label="ROM Directory">
      <Input
        name="rom-directory"
        id="rom-directory"
        placeholder="ROM Directory"
        bind:value={$romDirectory}
        on:change={onRomDirectoryChange}
      />
      <Tooltip target="rom-directory" placement="left">Root directory where ROMs will be stored</Tooltip>
    </FormGroup>
    <FormGroup floating label="TMP Directory">
      <Input
        name="tmp-directory"
        id="tmp-directory"
        placeholder="TMP Directory"
        bind:value={$tmpDirectory}
        on:change={onTmpDirectoryChange}
      />
      <Tooltip target="tmp-directory" placement="left">Temporary directory where ROMs will be extrated</Tooltip>
    </FormGroup>
    <FormGroup id="group-subsystems">
      <Input
        type="switch"
        label="Group Subsystems"
        bind:checked={$groupSubsystems}
        on:change={onGroupSubsystemsChange}
      />
      <Tooltip target="group-subsystems" placement="left"
        >Group subsystems in the main system directory (eg: PS3 DLCs and updates)</Tooltip
      >
    </FormGroup>
    <FormGroup id="one-regions-subfolders">
      <InputGroup>
        <InputGroupText class="w-25">1G1R Subfolders</InputGroupText>
        <Input
          type="select"
          label="1G1R Subfolders"
          bind:value={$oneRegionsSubfolders}
          on:change={onOneRegionsSubfoldersChange}
        >
          {#each subfolderSchemesChoices as subfolderSchemeChoice}
            <option value={subfolderSchemeChoice}>{subfolderSchemeChoice}</option>
          {/each}
        </Input>
      </InputGroup>
      <Tooltip target="one-regions-subfolders" placement="left">Store 1G1R games in subfolders</Tooltip>
    </FormGroup>
    <FormGroup id="all-regions-subfolders">
      <InputGroup>
        <InputGroupText class="w-25">All Subfolders</InputGroupText>
        <Input
          type="select"
          label="All Subfolders"
          bind:value={$allRegionsSubfolders}
          on:change={onAllRegionsSubfoldersChange}
        >
          {#each subfolderSchemesChoices as subfolderSchemeChoice}
            <option value={subfolderSchemeChoice}>{subfolderSchemeChoice}</option>
          {/each}
        </Input>
      </InputGroup>
      <Tooltip target="all-regions-subfolders" placement="left">Store all games in subfolders</Tooltip>
    </FormGroup>
  </ModalBody>
</Modal> -->
