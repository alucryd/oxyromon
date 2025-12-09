<script>
  import "../app.css";

  import { Navbar, NavBrand, NavHamburger, DarkMode, Button, ButtonGroup, Input } from "flowbite-svelte";
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

  // let isNavbarOpen = false;

  // function handleNavbarUpdate(event) {
  //   isNavbarOpen = event.detail.isOpen;
  // }

  function buttonClasses(color) {
    const classes = {
      blue: "text-base bg-blue-900 dark:bg-blue-900 hover:bg-blue-700 dark:hover:bg-blue-700",
      green: "text-base bg-green-900 dark:bg-green-900 hover:bg-green-700 dark:hover:bg-green-700",
      yellow: "text-base bg-yellow-900 dark:bg-yellow-900 hover:bg-yellow-700 dark:hover:bg-yellow-700",
      red: "text-base bg-red-900 dark:bg-red-900 hover:bg-red-700 dark:hover:bg-red-700",
      gray: "text-base bg-gray-900 dark:bg-gray-900 hover:bg-gray-700 dark:hover:bg-gray-700",
    };
    return classes[color] || "";
  }
</script>

<div class="flex min-h-screen">
  <Navbar fluid="true" class="fixed start-0 top-0 z-20 bg-gray-800 text-base text-white" expand="md">
    <NavBrand href="/" class="flex gap-2">
      <img src="/logo.svg" alt="logo" style="height: 32px;" />
      OXYROMON
    </NavBrand>
    <NavHamburger />
    <!-- <NavbarToggler onclick={() => (navbarIsOpen = !navbarIsOpen)} /> -->
    <!-- <Collapse
      isOpen={navbarIsOpen}
      navbar
      expand="md"
      class="d-flex justify-content-end"
      onupdate={handleNavbarUpdate}
    >
    </Collapse> -->
    <div class="grow"></div>
    <ButtonGroup class="mx-2">
      <Button
        color="blue"
        class={buttonClasses("blue")}
        bind:active={$oneRegionFilter}
        onclick={() => oneRegionFilter.update((b) => !b)}
      >
        {#if $oneRegionFilter}Show All{:else}Show 1G1R only{/if}
      </Button>
    </ButtonGroup>
    <ButtonGroup class="mx-2">
      <Button
        color="green"
        class={buttonClasses("green")}
        bind:active={$completeFilter}
        onclick={() => completeFilter.update((b) => !b)}
      >
        {#if $completeFilter}Hide{:else}Show{/if} Complete
      </Button>
      <Button
        color="yellow"
        class={buttonClasses("yellow")}
        bind:active={$incompleteFilter}
        onclick={() => incompleteFilter.update((b) => !b)}
      >
        {#if $incompleteFilter}Hide{:else}Show{/if} Incomplete
      </Button>
      <Button
        color="red"
        class={buttonClasses("red")}
        bind:active={$wantedFilter}
        onclick={() => wantedFilter.update((b) => !b)}
      >
        {#if $wantedFilter}Hide{:else}Show{/if} Wanted
      </Button>
      <Button
        color="gray"
        class={buttonClasses("gray")}
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
