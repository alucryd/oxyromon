<script>
  import { faGear } from "@fortawesome/free-solid-svg-icons";
  import "bootstrap/dist/css/bootstrap.min.css";
  import Fa from "svelte-fa";
  import {
    Button,
    ButtonGroup,
    ButtonToolbar,
    Collapse,
    Container,
    Input,
    InputGroup,
    Navbar,
    NavbarBrand,
    NavbarToggler,
  } from "sveltestrap";

  import SettingsModal from "../components/SettingsModal.svelte";
  import {
    completeFilter,
    ignoredFilter,
    incompleteFilter,
    isSettingsModalOpen,
    nameFilter,
    oneRegionFilter,
    wantedFilter,
  } from "../store.js";

  let navbarIsOpen = false;

  function handleNavbarUpdate(event) {
    navbarIsOpen = event.detail.isOpen;
  }
</script>

<div class="d-flex flex-column min-vh-100">
  <Navbar color="dark" dark sticky="top" expand="md" class="mb-3">
    <NavbarBrand href="/" class="ms-3">
      <img src="/logo.svg" alt="logo" style="height: 32px;" />
      oxyromon
    </NavbarBrand>
    <NavbarToggler on:click={() => (navbarIsOpen = !navbarIsOpen)} />
    <Collapse
      isOpen={navbarIsOpen}
      navbar
      expand="md"
      class="d-flex justify-content-end"
      on:update={handleNavbarUpdate}
    >
      <ButtonToolbar class="d-flex">
        <ButtonGroup class="ms-3">
          <Button color="primary" bind:active={$oneRegionFilter} on:click={() => oneRegionFilter.update((b) => !b)}>
            {#if $oneRegionFilter}Show All{:else}Show 1G1R only{/if}
          </Button>
        </ButtonGroup>
        <ButtonGroup class="ms-3">
          <Button color="success" bind:active={$completeFilter} on:click={() => completeFilter.update((b) => !b)}>
            {#if $completeFilter}Hide{:else}Show{/if} Complete
          </Button>
          <Button color="warning" bind:active={$incompleteFilter} on:click={() => incompleteFilter.update((b) => !b)}>
            {#if $incompleteFilter}Hide{:else}Show{/if} Incomplete
          </Button>
          <Button color="danger" bind:active={$wantedFilter} on:click={() => wantedFilter.update((b) => !b)}>
            {#if $wantedFilter}Hide{:else}Show{/if} Wanted
          </Button>
          <Button color="secondary" bind:active={$ignoredFilter} on:click={() => ignoredFilter.update((b) => !b)}>
            {#if $ignoredFilter}Hide{:else}Show{/if} Ignored
          </Button>
        </ButtonGroup>
        <InputGroup class="ms-3">
          <Input placeholder="Game Name" bind:value={$nameFilter} />
        </InputGroup>
        <ButtonGroup class="ms-3">
          <Button
            color="dark"
            bind:active={$isSettingsModalOpen}
            on:click={() => isSettingsModalOpen.update((b) => !b)}
          >
            <Fa icon={faGear} />
          </Button>
        </ButtonGroup>
      </ButtonToolbar>
    </Collapse>
  </Navbar>

  <Container fluid class="flex-fill">
    <slot />
  </Container>

  <SettingsModal toggle={() => isSettingsModalOpen.update((b) => !b)} />
</div>
