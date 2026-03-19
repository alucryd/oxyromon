<script>
  import "../app.css";

  import { Navbar, NavBrand, NavHamburger, DarkMode, Button, ButtonGroup, Input } from "flowbite-svelte";
  import { AdjustmentsHorizontalSolid, InfoCircleSolid, UploadSolid } from "flowbite-svelte-icons";

  import AboutModal from "../components/AboutModal.svelte";
  import ImportDatModal from "../components/ImportDatModal.svelte";
  import NotificationsButton from "../components/NotificationsButton.svelte";
  import SettingsModal from "../components/SettingsModal.svelte";
  import {
    completeFilter,
    ignoredFilter,
    incompleteFilter,
    isAboutModalOpen,
    isImportDatModalOpen,
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
        blue: "text-base bg-sky-800 dark:bg-sky-800 text-sky-300 hover:bg-sky-600 dark:hover:bg-sky-600",
        green:
          "text-base bg-emerald-800 dark:bg-emerald-800 text-emerald-300 hover:bg-emerald-600 dark:hover:bg-emerald-600",
        yellow: "text-base bg-amber-800 dark:bg-amber-800 text-amber-300 hover:bg-amber-600 dark:hover:bg-amber-600",
        red: "text-base bg-red-800 dark:bg-red-800 text-red-300 hover:bg-red-600 dark:hover:bg-red-600",
        gray: "text-base bg-slate-800 dark:bg-slate-800 text-slate-300 hover:bg-slate-600 dark:hover:bg-slate-600",
      };
      return (
        inactive[color] ||
        "text-sm bg-slate-800 dark:bg-slate-800 text-slate-300 hover:bg-slate-600 dark:hover:bg-slate-600"
      );
    }
    const classes = {
      blue: "text-base bg-sky-600 dark:bg-sky-600 text-sky-100 hover:bg-sky-400 dark:hover:bg-sky-400",
      green:
        "text-base bg-emerald-600 dark:bg-emerald-600 text-emerald-100 hover:bg-emerald-400 dark:hover:bg-emerald-400",
      yellow: "text-base bg-amber-600 dark:bg-amber-600 text-amber-100 hover:bg-amber-400 dark:hover:bg-amber-400",
      red: "text-base bg-red-600 dark:bg-red-600 text-red-100 hover:bg-red-400 dark:hover:bg-red-400",
      gray: "text-base bg-slate-600 dark:bg-slate-600 text-slate-100 hover:bg-slate-400 dark:hover:bg-slate-400",
    };
    return classes[color] || "text-base";
  }
</script>

<div class="flex min-h-screen">
  <Navbar fluid="true" class="fixed start-0 top-0 z-20 bg-slate-900 text-base text-white" expand="md">
    <NavBrand href="/" class="flex gap-2">
      <img src="/icon.svg" alt="OXYROMON" style="height: 40px;" />
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
    <div class="ml-4 flex items-center gap-1">
      <Button color="dark" class="p-2.5" title="Import DAT" onclick={() => isImportDatModalOpen.set(true)}>
        <UploadSolid />
      </Button>
    </div>
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
    <ButtonGroup class="mx-2">
      <Input class="text-base" placeholder="Game Name" bind:value={$nameFilter} />
    </ButtonGroup>
    <NotificationsButton />
    <ButtonGroup class="mx-2">
      <Button
        color="dark"
        class="p-2.5"
        bind:active={$isSettingsModalOpen}
        onclick={() => isSettingsModalOpen.update((b) => !b)}
      >
        <AdjustmentsHorizontalSolid />
      </Button>
      <Button
        color="dark"
        class="p-2.5"
        bind:active={$isAboutModalOpen}
        onclick={() => isAboutModalOpen.update((b) => !b)}
      >
        <InfoCircleSolid />
      </Button>
    </ButtonGroup>
    <DarkMode />
  </Navbar>

  <div class="flex w-full flex-col gap-4 bg-slate-200 dark:bg-slate-800">
    <slot />
  </div>

  <AboutModal />
  <ImportDatModal />
  <SettingsModal bind:open={$isSettingsModalOpen} />
</div>
