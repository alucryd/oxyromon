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

  let isNavbarOpen = false;

  function handleNavbarUpdate(event) {
    isNavbarOpen = event.detail.isOpen;
  }
</script>

<div class="flex min-h-screen">
  <Navbar fluid="true" class="fixed start-0 top-0 z-20 bg-gray-800 text-base text-white" expand="md">
    <NavBrand href="/" class="flex gap-2">
      <img src="/logo.svg" alt="logo" style="height: 32px;" />
      OXYROMON
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
    <div class="grow"></div>
    <ButtonGroup class="mx-2">
      <Button
        outline
        color="dark"
        class="bg-blue-900 text-base text-blue-300 hover:bg-blue-700 dark:text-blue-300"
        bind:active={$oneRegionFilter}
        onclick={() => oneRegionFilter.update((b) => !b)}
      >
        {#if $oneRegionFilter}Show All{:else}Show 1G1R only{/if}
      </Button>
    </ButtonGroup>
    <ButtonGroup class="mx-2">
      <Button
        outline
        color="dark"
        class="bg-green-900 text-base text-green-300 hover:bg-green-700 dark:text-green-300"
        bind:active={$completeFilter}
        onclick={() => completeFilter.update((b) => !b)}
      >
        {#if $completeFilter}Hide{:else}Show{/if} Complete
      </Button>
      <Button
        outline
        color="dark"
        class="bg-yellow-900 text-base text-yellow-300 hover:bg-yellow-700 dark:text-yellow-300"
        bind:active={$incompleteFilter}
        onclick={() => incompleteFilter.update((b) => !b)}
      >
        {#if $incompleteFilter}Hide{:else}Show{/if} Incomplete
      </Button>
      <Button
        outline
        color="dark"
        class="bg-red-900 text-base text-red-300 hover:bg-red-700 dark:text-red-300"
        bind:active={$wantedFilter}
        onclick={() => wantedFilter.update((b) => !b)}
      >
        {#if $wantedFilter}Hide{:else}Show{/if} Wanted
      </Button>
      <Button
        outline
        color="dark"
        class="bg-gray-900 text-base text-gray-300 hover:bg-gray-700 dark:text-gray-300"
        bind:active={$ignoredFilter}
        onclick={() => ignoredFilter.update((b) => !b)}
      >
        {#if $ignoredFilter}Hide{:else}Show{/if} Ignored
      </Button>
    </ButtonGroup>
    <ButtonGroup class="mx-2 h-10">
      <Input class="text-base" placeholder="Game Name" bind:value={$nameFilter} />
    </ButtonGroup>
    <ButtonGroup>
      <Button
        color="dark"
        class="ml-2 p-2.5"
        bind:active={$isSettingsModalOpen}
        onclick={() => isSettingsModalOpen.update((b) => !b)}
      >
        <AdjustmentsHorizontalSolid />
      </Button>
    </ButtonGroup>
    <DarkMode />
  </Navbar>

  <div class="flex w-full flex-col gap-4 bg-gray-300">
    <slot />
  </div>

  <SettingsModal />
</div>
