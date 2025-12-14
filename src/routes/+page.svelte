<script>
  import { uniq } from "lodash-es";
  import prettyBytes from "pretty-bytes";
  import { onMount, onDestroy } from "svelte";
  import {
    Card,
    Tooltip,
    Button,
    Modal,
    Toast,
    Table,
    TableHead,
    TableHeadCell,
    TableBody,
    TableBodyRow,
    TableBodyCell,
  } from "flowbite-svelte";
  import {
    ChevronLeftOutline,
    ChevronRightOutline,
    ChevronDoubleLeftOutline,
    ChevronDoubleRightOutline,
    TrashBinOutline,
    ExclamationCircleOutline,
    CheckCircleSolid,
    CloseCircleSolid,
  } from "flowbite-svelte-icons";
  import { Spinner } from "flowbite-svelte";

  import { purgeSystem } from "../mutation.js";
  import { connectSSE, disconnectSSE } from "../events.js";
  import {
    getGamesBySystemId,
    getRomsByGameIdAndSystemId,
    getSizesBySystemId,
    getSettings,
    getSystems,
    updateGames,
    updateRoms,
    updateRomfiles,
    updateSystems,
  } from "../query.js";
  import {
    completeFilter,
    gameId,
    games,
    gamesPage,
    gamesTotalPages,
    ignoredFilter,
    incompleteFilter,
    nameFilter,
    oneRegionActualSize,
    oneRegionFilter,
    oneRegionOriginalSize,
    pageSize,
    romfiles,
    romfilesPage,
    romfilesTotalPages,
    roms,
    romsPage,
    romsTotalPages,
    systemId,
    purgingSystemId,
    systems,
    systemsPage,
    systemsTotalPages,
    totalActualSize,
    totalOriginalSize,
    unfilteredGames,
    unfilteredRoms,
    unfilteredSystems,
    wantedFilter,
    loadingSystems,
    loadingGames,
    loadingRoms,
    loadingSizes,
    romsPageSize,
  } from "../store.js";

  $: systemsFirstPage = $systemsPage == 1;
  $: systemsLastPage = $systemsPage == $systemsTotalPages;
  $: gamesFirstPage = $gamesPage == 1;
  $: gamesLastPage = $gamesPage == $gamesTotalPages;
  $: romsFirstPage = $romsPage == 1;
  $: romsLastPage = $romsPage == $romsTotalPages;
  $: romfilesFirstPage = $romfilesPage == 1;
  $: romfilesLastPage = $romfilesPage == $romfilesTotalPages;

  function computeSystemColor(system) {
    if (system.completion == 2) {
      return "dark:text-green-300 text-green-500";
    }

    if (system.completion == 1) {
      return "dark:text-yellow-300 text-yellow-500";
    }

    return "dark:text-red-300 text-red-500";
  }

  function computeGameColor(game) {
    if (game.sorting == 2) {
      return "dark:text-gray-300 text-gray-500";
    }

    if (game.completion == 2) {
      return "dark:text-green-300 text-green-500";
    }

    if (game.completion == 1) {
      return "dark:text-yellow-300 text-yellow-500";
    }

    return "dark:text-red-300 text-red-500";
  }

  function computeRomColor(rom) {
    if (rom.ignored) {
      return "dark:text-gray-300 text-gray-500";
    }

    if (rom.romfile) {
      return "dark:text-green-300 text-green-500";
    }

    return "dark:text-red-300 text-red-500";
  }

  let deleteModalOpen = false;
  let systemToDelete = null;
  let toastMessage = "";
  let toastType = "success"; // 'success' or 'error'
  let showToast = false;

  const onPurgeSystemClick = (system) => {
    systemToDelete = system;
    deleteModalOpen = true;
  };

  const confirmDelete = async () => {
    if (systemToDelete) {
      await purgeSystem(systemToDelete.id);
    }
    deleteModalOpen = false;
    systemToDelete = null;
  };

  const cancelDelete = () => {
    deleteModalOpen = false;
    systemToDelete = null;
  };

  const showToastNotification = (message, type) => {
    toastMessage = message;
    toastType = type;
    showToast = true;
    const duration = type === "info" ? 3000 : 5000;
    setTimeout(() => (showToast = false), duration);
  };

  onMount(async () => {
    // Connect to SSE endpoint
    connectSSE(showToastNotification);

    await getSettings();
    systemsPage.subscribe(async () => {
      await updateSystems();
    });
    systemId.subscribe(async (systemId) => {
      gameId.set(-1);
      gamesPage.set(1);
      games.set([]);
      roms.set([]);
      romfiles.set([]);
      await getGamesBySystemId(systemId);
      await getSizesBySystemId(systemId);
    });
    gamesPage.subscribe(async () => {
      await updateGames();
    });
    gameId.subscribe(async (gameId) => {
      romsPage.set(1);
      romfilesPage.set(1);
      roms.set([]);
      romfiles.set([]);
      await getRomsByGameIdAndSystemId(gameId, $systemId);
    });
    romsPage.subscribe(async () => {
      await updateRoms();
    });
    romfilesPage.subscribe(async () => {
      await updateRomfiles();
    });
    pageSize.subscribe(async () => {
      await updateSystems();
      await updateGames();
    });
    romsPageSize.subscribe(async () => {
      await updateRoms();
      await updateRomfiles();
    });
    completeFilter.subscribe(async () => {
      if ($gamesPage != 1) {
        gamesPage.set(1);
      } else {
        await updateGames();
      }
    });
    incompleteFilter.subscribe(async () => {
      if ($gamesPage != 1) {
        gamesPage.set(1);
      } else {
        await updateGames();
      }
    });
    wantedFilter.subscribe(async () => {
      if ($gamesPage != 1) {
        gamesPage.set(1);
      } else {
        await updateGames();
      }
    });
    ignoredFilter.subscribe(async () => {
      if ($gamesPage != 1) {
        gamesPage.set(1);
      } else {
        await updateGames();
      }
    });
    oneRegionFilter.subscribe(async () => {
      if ($gamesPage != 1) {
        gamesPage.set(1);
      } else {
        await updateGames();
      }
    });
    nameFilter.subscribe(async () => {
      if ($gamesPage != 1) {
        gamesPage.set(1);
      } else {
        await updateGames();
      }
    });
    await getSystems();
  });

  onDestroy(() => {
    disconnectSSE();
  });
</script>

<div class="flex min-h-screen w-full flex-col px-4">
  <div class="mt-20 mb-4 grid flex-1 grid-cols-1 gap-4 md:grid-cols-10">
    <div class="flex flex-col md:col-span-2">
      <Card class="flex max-w-none flex-1 flex-col overflow-hidden">
        <Table hoverable striped border={false} class="mb-4 table-fixed">
          <TableHead class="text-left text-base">
            <TableHeadCell class="w-full">Systems</TableHeadCell>
            <TableHeadCell class="w-1">
              {#if $loadingSystems}
                <Spinner size="4" />
              {/if}
            </TableHeadCell>
          </TableHead>
          <TableBody>
            {#each $systems as system, i (system.id)}
              <TableBodyRow>
                <TableBodyCell id="tbc-system-{i}" class="p-0 {system.id == $systemId ? 'active' : ''}">
                  <button
                    class="block w-full truncate px-4 py-2 text-left text-base {computeSystemColor(system)}"
                    onclick={() => {
                      systemId.set(system.id);
                    }}
                  >
                    {system.name}
                  </button>
                </TableBodyCell>
                <TableBodyCell class="px-2 py-2 text-right">
                  {#if purgingSystemId === system.id}
                    <Spinner size="4" />
                  {:else}
                    <TrashBinOutline
                      class="h-4 w-4 cursor-pointer text-red-600 hover:text-red-800"
                      onclick={() => onPurgeSystemClick(system)}
                    />
                  {/if}
                </TableBodyCell>
                {#if system.description && system.description != system.name}
                  <Tooltip triggeredBy="#tbc-system-{i}" placement="bottom">{system.description}</Tooltip>
                {/if}
              </TableBodyRow>
            {/each}
          </TableBody>
        </Table>
        <div class="m-4 mt-auto flex items-center justify-center gap-2">
          <Button
            size="xs"
            color="alternative"
            class="px-2"
            disabled={systemsFirstPage}
            onclick={() => systemsPage.set(1)}
          >
            <ChevronDoubleLeftOutline class="h-4 w-4" />
          </Button>
          <Button
            size="xs"
            color="alternative"
            class="px-2"
            disabled={systemsFirstPage}
            onclick={() => systemsPage.update((n) => n - 1)}
          >
            <ChevronLeftOutline class="h-4 w-4" />
          </Button>
          <span class="w-full px-3 py-1 text-center text-sm text-gray-700 dark:text-gray-300">
            {$systemsPage} / {$systemsTotalPages}
          </span>
          <Button
            size="xs"
            color="alternative"
            class="px-2"
            disabled={systemsLastPage}
            onclick={() => systemsPage.update((n) => n + 1)}
          >
            <ChevronRightOutline class="h-4 w-4" />
          </Button>
          <Button
            size="xs"
            color="alternative"
            class="px-2"
            disabled={systemsLastPage}
            onclick={() => systemsPage.set($systemsTotalPages)}
          >
            <ChevronDoubleRightOutline class="h-4 w-4" />
          </Button>
        </div>
      </Card>
    </div>
    <div class="flex flex-col md:col-span-3">
      <Card class="flex max-w-none flex-1 flex-col overflow-hidden">
        <Table hoverable striped border={false} class="mb-4 table-fixed">
          <TableHead class="text-left text-base">
            <TableHeadCell class="w-full">Games</TableHeadCell>
            <TableHeadCell class="w-1">
              {#if $loadingGames}
                <Spinner size="4" />
              {/if}
            </TableHeadCell>
          </TableHead>
          <TableBody>
            {#each $games as game, i (game.id)}
              <TableBodyRow>
                <TableBodyCell
                  colspan="2"
                  id="tbc-game-{i}"
                  class="p-0 {game.sorting == 1 ? 'font-bold' : ''} {game.id == $gameId ? 'active' : ''}"
                >
                  <button
                    class="block w-full truncate px-4 py-2 text-left text-base {computeGameColor(game)}"
                    onclick={() => {
                      gameId.set(game.id);
                    }}
                  >
                    {game.name}
                  </button>
                </TableBodyCell>
                {#if game.description && game.description != game.name}
                  <Tooltip triggeredBy="#tbc-game-{i}" placement="bottom">{game.description}</Tooltip>
                {/if}
              </TableBodyRow>
            {/each}
          </TableBody>
        </Table>
        <div class="m-4 mt-auto flex items-center justify-center gap-2">
          <Button size="xs" color="alternative" class="px-2" disabled={gamesFirstPage} onclick={() => gamesPage.set(1)}>
            <ChevronDoubleLeftOutline class="h-4 w-4" />
          </Button>
          <Button
            size="xs"
            color="alternative"
            class="px-2"
            disabled={gamesFirstPage}
            onclick={() => gamesPage.update((n) => n - 1)}
          >
            <ChevronLeftOutline class="h-4 w-4" />
          </Button>
          <span class="w-full px-3 py-1 text-center text-sm text-gray-700 dark:text-gray-300">
            {$gamesPage} / {$gamesTotalPages}
          </span>
          <Button
            size="xs"
            color="alternative"
            class="px-2"
            disabled={gamesLastPage}
            onclick={() => gamesPage.update((n) => n + 1)}
          >
            <ChevronRightOutline class="h-4 w-4" />
          </Button>
          <Button
            size="xs"
            color="alternative"
            class="px-2"
            disabled={gamesLastPage}
            onclick={() => gamesPage.set($gamesTotalPages)}
          >
            <ChevronDoubleRightOutline class="h-4 w-4" />
          </Button>
        </div>
      </Card>
    </div>
    <div class="flex flex-col gap-4 md:col-span-5">
      <Card class="flex max-w-none flex-1 flex-col overflow-hidden">
        <Table hoverable striped border={false} class="mb-4 table-fixed">
          <TableHead class="text-left text-base">
            <TableHeadCell>Roms</TableHeadCell>
            <TableHeadCell class="w-1">
              {#if $loadingRoms}
                <Spinner size="4" />
              {/if}
            </TableHeadCell>
          </TableHead>
          <TableBody>
            {#each $roms as rom (rom.id)}
              <TableBodyRow>
                <TableBodyCell colspan="2" class="truncate px-4 py-2 text-left text-base {computeRomColor(rom)}">
                  {rom.name}
                </TableBodyCell>
              </TableBodyRow>
            {/each}
          </TableBody>
        </Table>
        <div class="m-4 mt-auto flex items-center justify-center gap-2">
          <Button size="xs" color="alternative" class="px-2" disabled={romsFirstPage} onclick={() => romsPage.set(1)}>
            <ChevronDoubleLeftOutline class="h-4 w-4" />
          </Button>
          <Button
            size="xs"
            color="alternative"
            class="px-2"
            disabled={romsFirstPage}
            onclick={() => romsPage.update((n) => n - 1)}
          >
            <ChevronLeftOutline class="h-4 w-4" />
          </Button>
          <span class="w-full px-3 py-1 text-center text-sm text-gray-700 dark:text-gray-300">
            {$romsPage} / {$romsTotalPages}
          </span>
          <Button
            size="xs"
            color="alternative"
            class="px-2"
            disabled={romsLastPage}
            onclick={() => romsPage.update((n) => n + 1)}
          >
            <ChevronRightOutline class="h-4 w-4" />
          </Button>
          <Button
            size="xs"
            color="alternative"
            class="px-2"
            disabled={romsLastPage}
            onclick={() => romsPage.set($romsTotalPages)}
          >
            <ChevronDoubleRightOutline class="h-4 w-4" />
          </Button>
        </div>
      </Card>
      <Card class="flex max-w-none flex-1 flex-col overflow-hidden">
        <Table hoverable striped border={false} class="mb-4 table-fixed">
          <TableHead class="text-left text-base">
            <TableHeadCell>Romfiles</TableHeadCell>
            <TableHeadCell class="w-1">
              {#if $loadingRoms}
                <Spinner size="4" />
              {/if}
            </TableHeadCell>
          </TableHead>
          <TableBody>
            {#each $romfiles as romfile (romfile.path)}
              <TableBodyRow>
                <TableBodyCell colspan="2" class="truncate px-4 py-2 text-left text-base">
                  {romfile.path.split("/").slice(1).join("/")}
                </TableBodyCell>
              </TableBodyRow>
            {/each}
          </TableBody>
        </Table>
        <div class="m-4 mt-auto flex items-center justify-center gap-2">
          <Button
            size="xs"
            color="alternative"
            class="px-2"
            disabled={romfilesFirstPage}
            onclick={() => romfilesPage.set(1)}
          >
            <ChevronDoubleLeftOutline class="h-4 w-4" />
          </Button>
          <Button
            size="xs"
            color="alternative"
            class="px-2"
            disabled={romfilesFirstPage}
            onclick={() => romfilesPage.update((n) => n - 1)}
          >
            <ChevronLeftOutline class="h-4 w-4" />
          </Button>
          <span class="w-full px-3 py-1 text-center text-sm text-gray-700 dark:text-gray-300">
            {$romfilesPage} / {$romfilesTotalPages}
          </span>
          <Button
            size="xs"
            color="alternative"
            class="px-2"
            disabled={romfilesLastPage}
            onclick={() => romfilesPage.update((n) => n + 1)}
          >
            <ChevronRightOutline class="h-4 w-4" />
          </Button>
          <Button
            size="xs"
            color="alternative"
            class="px-2"
            disabled={romfilesLastPage}
            onclick={() => romfilesPage.set($romfilesTotalPages)}
          >
            <ChevronDoubleRightOutline class="h-4 w-4" />
          </Button>
        </div>
      </Card>
    </div>
  </div>
  <div class="mb-4">
    <Card class="max-w-none overflow-hidden">
      <Table border={false} class="table-fixed">
        <TableHead class="text-left text-base">
          <TableHeadCell colspan="4">Statistics</TableHeadCell>
        </TableHead>
        <TableBody class="text-left text-base font-medium">
          <TableBodyRow>
            <TableBodyCell>
              <span>Systems:</span>
              {#if $loadingSystems}
                <Spinner size="4" />
              {:else}
                {$unfilteredSystems.length}
              {/if}
            </TableBodyCell>
            <TableBodyCell>
              <span>Games:</span>
              {#if $loadingGames}
                <Spinner size="4" />
              {:else}
                {$unfilteredGames.length}
              {/if}
            </TableBodyCell>
            <TableBodyCell>
              <span>Roms:</span>
              {#if $loadingRoms}
                <Spinner size="4" />
              {:else}
                {$unfilteredRoms.length}
              {/if}
            </TableBodyCell>
            <TableBodyCell>
              <span>Romfiles:</span>
              {#if $loadingRoms}
                <Spinner size="4" />
              {:else}
                {uniq($unfilteredRoms.filter((rom) => rom.romfile).map((rom) => rom.romfile.path)).length}
              {/if}
            </TableBodyCell>
          </TableBodyRow>
          <TableBodyRow>
            <TableBodyCell>
              <span>Total Original Size:</span>
              {#if $loadingSizes}
                <Spinner size="4" />
              {:else}
                {prettyBytes($totalOriginalSize)}
              {/if}
            </TableBodyCell>
            <TableBodyCell>
              <span>1G1R Original Size:</span>
              {#if $loadingSizes}
                <Spinner size="4" />
              {:else}
                {prettyBytes($oneRegionOriginalSize)}
              {/if}
            </TableBodyCell>
            <TableBodyCell>
              <span>Total Actual Size:</span>
              {#if $loadingSizes}
                <Spinner size="4" />
              {:else}
                {prettyBytes($totalActualSize)}
              {/if}
            </TableBodyCell>
            <TableBodyCell>
              <span>1G1R Actual Size:</span>
              {#if $loadingSizes}
                <Spinner size="4" />
              {:else}
                {prettyBytes($oneRegionActualSize)}
              {/if}
            </TableBodyCell>
          </TableBodyRow>
        </TableBody>
      </Table>
    </Card>
  </div>

  <Modal bind:open={deleteModalOpen} size="xs" autoclose={false}>
    <div class="text-center">
      <ExclamationCircleOutline class="mx-auto mb-4 h-12 w-12 text-gray-400 dark:text-gray-200" />
      <h3 class="mb-5 text-lg font-normal text-gray-500 dark:text-gray-400">
        Are you sure you want to delete system "{systemToDelete?.name}"?
      </h3>
      <p class="mb-5 text-sm text-gray-400 dark:text-gray-500">
        This action cannot be undone. All data associated with this system will be permanently removed.
      </p>
      <Button color="red" class="me-2" onclick={confirmDelete}>Yes, I'm sure</Button>
      <Button color="alternative" onclick={cancelDelete}>No, cancel</Button>
    </div>
  </Modal>

  {#if showToast}
    <Toast
      color={toastType === "success" ? "green" : toastType === "error" ? "red" : "blue"}
      position="bottom-right"
      class="fixed right-4 bottom-4 z-50"
    >
      <svelte:fragment slot="icon">
        {#if toastType === "success"}
          <CheckCircleSolid class="h-5 w-5" />
        {:else if toastType === "error"}
          <CloseCircleSolid class="h-5 w-5" />
        {:else}
          <ExclamationCircleOutline class="h-5 w-5" />
        {/if}
      </svelte:fragment>
      {toastMessage}
    </Toast>
  {/if}
</div>
