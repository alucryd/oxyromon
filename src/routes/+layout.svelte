<script>
  import "../app.css";

  import { Navbar, NavBrand, NavLi, NavUl, NavHamburger, DarkMode, Button, ButtonGroup, Input } from "flowbite-svelte";
  import { AdjustmentsHorizontalSolid } from "flowbite-svelte-icons";

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

<div class="flex min-h-screen">
  <Navbar fluid="true" class="fixed start-0 top-0 z-20 bg-gray-800 text-white" expand="md">
    <NavBrand href="/" class="flex gap-2">
      <img src="/logo.svg" alt="logo" style="height: 32px;" />
      oxyromon
    </NavBrand>
    <NavHamburger />
    <!-- <NavbarToggler on:click={() => (navbarIsOpen = !navbarIsOpen)} /> -->
    <!-- <Collapse
      isOpen={navbarIsOpen}
      navbar
      expand="md"
      class="d-flex justify-content-end"
      on:update={handleNavbarUpdate}
    >
    </Collapse> -->
    <div class="flex-grow" />
    <ButtonGroup class="mx-1">
      <Button color="blue" bind:active={$oneRegionFilter} onclick={() => oneRegionFilter.update((b) => !b)}>
        {#if $oneRegionFilter}Show All{:else}Show 1G1R only{/if}
      </Button>
    </ButtonGroup>
    <ButtonGroup class="mx-1">
      <Button color="green" bind:active={$completeFilter} onclick={() => completeFilter.update((b) => !b)}>
        {#if $completeFilter}Hide{:else}Show{/if} Complete
      </Button>
      <Button
        color="yellow"
        class="text-black"
        bind:active={$incompleteFilter}
        onclick={() => incompleteFilter.update((b) => !b)}
      >
        {#if $incompleteFilter}Hide{:else}Show{/if} Incomplete
      </Button>
      <Button color="red" bind:active={$wantedFilter} onclick={() => wantedFilter.update((b) => !b)}>
        {#if $wantedFilter}Hide{:else}Show{/if} Wanted
      </Button>
      <Button color="gray" bind:active={$ignoredFilter} onclick={() => ignoredFilter.update((b) => !b)}>
        {#if $ignoredFilter}Hide{:else}Show{/if} Ignored
      </Button>
    </ButtonGroup>
    <ButtonGroup class="mx-1">
      <Input placeholder="Game Name" bind:value={$nameFilter} />
    </ButtonGroup>
    <ButtonGroup class="mx-1">
      <Button color="dark" bind:active={$isSettingsModalOpen} onclick={() => isSettingsModalOpen.update((b) => !b)}>
        <AdjustmentsHorizontalSolid />
      </Button>
    </ButtonGroup>
    <DarkMode class="mx-1" />
  </Navbar>

  <!-- <Container fluid class="flex-fill">
    <slot />
  </Container> -->

  <SettingsModal toggle={() => isSettingsModalOpen.update((b) => !b)} />
</div>
