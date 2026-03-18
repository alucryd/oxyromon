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

  function buttonClasses(color, active) {
    if (!active) {
      const inactive = {
        blue: "text-sm bg-sky-800 text-sky-300 hover:bg-sky-700",
        green: "text-sm bg-slate-700 text-slate-400 hover:bg-slate-600",
        yellow: "text-sm bg-slate-700 text-slate-400 hover:bg-slate-600",
        red: "text-sm bg-slate-700 text-slate-400 hover:bg-slate-600",
        gray: "text-sm bg-slate-700 text-slate-400 hover:bg-slate-600",
      };
      return inactive[color] || "text-sm bg-slate-700 text-slate-400 hover:bg-slate-600";
    }
    const classes = {
      blue: "text-sm bg-sky-600 text-white hover:bg-sky-500",
      green: "text-sm bg-emerald-700 text-white hover:bg-emerald-600",
      yellow: "text-sm bg-amber-600 text-white hover:bg-amber-500",
      red: "text-sm bg-rose-700 text-white hover:bg-rose-600",
      gray: "text-sm bg-slate-500 text-white hover:bg-slate-400",
    };
    return classes[color] || "text-sm";
  }
</script>

<div class="flex min-h-screen">
  <Navbar fluid="true" class="fixed start-0 top-0 z-20 bg-slate-900 text-base text-white" expand="md">
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
        class={buttonClasses("blue", $oneRegionFilter)}
        onclick={() => oneRegionFilter.update((b) => !b)}
      >
        {#if $oneRegionFilter}Show All{:else}Show 1G1R only{/if}
      </Button>
    </ButtonGroup>
    <ButtonGroup class="mx-2">
      <Button
        color="green"
        class={buttonClasses("green", $completeFilter)}
        onclick={() => completeFilter.update((b) => !b)}
      >
        {#if $completeFilter}Hide{:else}Show{/if} Complete
      </Button>
      <Button
        color="yellow"
        class={buttonClasses("yellow", $incompleteFilter)}
        onclick={() => incompleteFilter.update((b) => !b)}
      >
        {#if $incompleteFilter}Hide{:else}Show{/if} Incomplete
      </Button>
      <Button color="red" class={buttonClasses("red", $wantedFilter)} onclick={() => wantedFilter.update((b) => !b)}>
        {#if $wantedFilter}Hide{:else}Show{/if} Wanted
      </Button>
      <Button
        color="gray"
        class={buttonClasses("gray", $ignoredFilter)}
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

  <div class="flex w-full flex-col gap-4 bg-slate-200 dark:bg-slate-800">
    <slot />
  </div>

  <SettingsModal bind:open={$isSettingsModalOpen} />
</div>
