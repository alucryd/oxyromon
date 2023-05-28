<script>
  import { faTimes } from "@fortawesome/free-solid-svg-icons";
  import Fa from "svelte-fa";
  import {
    Badge,
    Button,
    Col,
    FormGroup,
    Input,
    InputGroup,
    InputGroupText,
    Modal,
    ModalBody,
    ModalHeader,
    Row,
    Tooltip,
  } from "sveltestrap";

  import { addToList, removeFromList, setBool, setPreferRegions, setPreferVersions } from "../mutation.js";
  import { getSettings } from "../query.js";
  import {
    allRegions,
    allRegionsKey,
    discardFlags,
    discardFlagsKey,
    discardReleases,
    discardReleasesKey,
    isSettingsModalOpen,
    languages,
    languagesKey,
    oneRegions,
    oneRegionsKey,
    preferFlags,
    preferFlagsKey,
    preferParents,
    preferParentsKey,
    preferRegions,
    preferRegionsChoices,
    preferVersions,
    preferVersionsChoices,
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
</script>

<Modal isOpen={$isSettingsModalOpen} {toggle} size="xl" class="text-start">
  <ModalHeader {toggle}>Settings</ModalHeader>
  <ModalBody class="pb-0">
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
              <Fa icon={faTimes} />
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
      <Tooltip target="one-regions" placement="top">2 letters, uppercase, ordered</Tooltip>
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
              <Fa icon={faTimes} />
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
      <Tooltip target="all-regions" placement="top">2 letters, uppercase, unordered</Tooltip>
    </FormGroup>
    <Row class="mb-2">
      <Col>
        {#each $languages as language}
          <Badge class="m-1">
            {language}
            <Button
              small
              class="px-1 py-0 text-center"
              on:click={() => onRemoveFromListClick(languagesKey, language)}
            >
              <Fa icon={faTimes} />
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
      <Tooltip target="languages" placement="top">2 letters, capitalized</Tooltip>
    </FormGroup>
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
              <Fa icon={faTimes} />
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
              <Fa icon={faTimes} />
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
    </FormGroup>
    <FormGroup>
      <Input
        id="prefer-parents"
        type="switch"
        label="Prefer Parents"
        bind:checked={$preferParents}
        on:change={() => onSwitchChange(preferParentsKey, !$preferParents)}
      />
    </FormGroup>
    <FormGroup>
      <InputGroup>
        <InputGroupText>Prefer Regions</InputGroupText>
        <Input
          id="prefer-regions"
          type="select"
          label="Prefer Regions"
          bind:value={$preferRegions}
          on:change={onPreferRegionsChange}
        >
          {#each preferRegionsChoices as preferRegionChoice}
            <option value={preferRegionChoice}>{preferRegionChoice}</option>
          {/each}
      </Input>
      </InputGroup>
    </FormGroup>
    <FormGroup>
      <InputGroup>
        <InputGroupText>Prefer Versions</InputGroupText>
        <Input
          id="prefer-versions"
          type="select"
          label="Prefer Versions"
          bind:value={$preferVersions}
          on:change={onPreferVersionsChange}
        >
          {#each preferVersionsChoices as preferVersionChoice}
            <option value={preferVersionChoice}>{preferVersionChoice}</option>
          {/each}
        </Input>
      </InputGroup>
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
              <Fa icon={faTimes} />
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
    </FormGroup>
  </ModalBody>
</Modal>
