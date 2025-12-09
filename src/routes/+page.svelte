<script>
  import { uniq } from "lodash-es";
  import prettyBytes from "pretty-bytes";
  import { onMount } from "svelte";
  import {
    Card,
    Tooltip,
    Button,
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
  } from "flowbite-svelte-icons";
  import { Spinner } from "flowbite-svelte";

  import { purgeSystem } from "../mutation.js";
  import {
    getGamesBySystemId,
    getRomsByGameIdAndSystemId,
    getSizesBySystemId,
    getSettings,
    getSystems,
    updateGames,
    updateRoms,
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
  } from "../store.js";

  $: systemsFirstPage = $systemsPage == 1;
  $: systemsLastPage = $systemsPage == $systemsTotalPages;
  $: gamesFirstPage = $gamesPage == 1;
  $: gamesLastPage = $gamesPage == $gamesTotalPages;
  $: romsFirstPage = $romsPage == 1;
  $: romsLastPage = $romsPage == $romsTotalPages;

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

  const onPurgeSystemClick = async (systemId) => {
    await purgeSystem(systemId);
    await getSystems();
  };

  onMount(async () => {
    await getSettings();
    systemsPage.subscribe(async () => {
      await updateSystems();
    });
    systemId.subscribe(async (systemId) => {
      gameId.set(-1);
      gamesPage.set(1);
      if (systemId === -1) {
        games.set([]);
      } else {
        await getGamesBySystemId(systemId);
        await getSizesBySystemId(systemId);
      }
    });
    gamesPage.subscribe(async () => {
      await updateGames();
    });
    gameId.subscribe(async (gameId) => {
      romsPage.set(1);
      if (gameId === -1) {
        roms.set([]);
      } else {
        await getRomsByGameIdAndSystemId(gameId, $systemId);
      }
    });
    romsPage.subscribe(async () => {
      await updateRoms();
    });
    pageSize.subscribe(async () => {
      await updateSystems();
      await updateGames();
      await updateRoms();
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
</script>

<div class="w-full px-4">
  <div class="mt-20 mb-4 grid grid-cols-1 gap-4 md:grid-cols-10">
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
            {#each $systems as system, i}
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
                      onclick={() => onPurgeSystemClick(system.id)}
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
          <Button size="sm" color="alternative" disabled={systemsFirstPage} onclick={() => systemsPage.set(1)}>
            <ChevronDoubleLeftOutline class="h-4 w-4" />
          </Button>
          <Button
            size="sm"
            color="alternative"
            disabled={systemsFirstPage}
            onclick={() => systemsPage.update((n) => n - 1)}
          >
            <ChevronLeftOutline class="h-4 w-4" />
          </Button>
          <span class="px-3 py-1 text-sm text-gray-700 dark:text-gray-300">
            {$systemsPage} / {$systemsTotalPages}
          </span>
          <Button
            size="sm"
            color="alternative"
            disabled={systemsLastPage}
            onclick={() => systemsPage.update((n) => n + 1)}
          >
            <ChevronRightOutline class="h-4 w-4" />
          </Button>
          <Button
            size="sm"
            color="alternative"
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
            {#each $games as game, i}
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
          <Button size="sm" color="alternative" disabled={gamesFirstPage} onclick={() => gamesPage.set(1)}>
            <ChevronDoubleLeftOutline class="h-4 w-4" />
          </Button>
          <Button
            size="sm"
            color="alternative"
            disabled={gamesFirstPage}
            onclick={() => gamesPage.update((n) => n - 1)}
          >
            <ChevronLeftOutline class="h-4 w-4" />
          </Button>
          <span class="px-3 py-1 text-sm text-gray-700 dark:text-gray-300">
            {$gamesPage} / {$gamesTotalPages}
          </span>
          <Button size="sm" color="alternative" disabled={gamesLastPage} onclick={() => gamesPage.update((n) => n + 1)}>
            <ChevronRightOutline class="h-4 w-4" />
          </Button>
          <Button
            size="sm"
            color="alternative"
            disabled={gamesLastPage}
            onclick={() => gamesPage.set($gamesTotalPages)}
          >
            <ChevronDoubleRightOutline class="h-4 w-4" />
          </Button>
        </div>
      </Card>
    </div>
    <div class="flex flex-col md:col-span-5">
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
            {#each $roms as rom}
              <TableBodyRow>
                <TableBodyCell colspan="2" class="truncate px-4 py-2 text-left text-base {computeRomColor(rom)}">
                  {rom.name}
                </TableBodyCell>
              </TableBodyRow>
            {/each}
          </TableBody>
        </Table>
        <div class="m-4 mt-auto flex items-center justify-center gap-2">
          <Button size="sm" color="alternative" disabled={romsFirstPage} onclick={() => romsPage.set(1)}>
            <ChevronDoubleLeftOutline class="h-4 w-4" />
          </Button>
          <Button size="sm" color="alternative" disabled={romsFirstPage} onclick={() => romsPage.update((n) => n - 1)}>
            <ChevronLeftOutline class="h-4 w-4" />
          </Button>
          <span class="px-3 py-1 text-sm text-gray-700 dark:text-gray-300">
            {$romsPage} / {$romsTotalPages}
          </span>
          <Button size="sm" color="alternative" disabled={romsLastPage} onclick={() => romsPage.update((n) => n + 1)}>
            <ChevronRightOutline class="h-4 w-4" />
          </Button>
          <Button size="sm" color="alternative" disabled={romsLastPage} onclick={() => romsPage.set($romsTotalPages)}>
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
        <TableBody class="text-left text-base">
          <TableBodyRow>
            <TableBodyCell>
              <span class="font-medium">Systems:</span>
              {#if $loadingSystems}
                <Spinner size="4" />
              {:else}
                {$unfilteredSystems.length}
              {/if}
            </TableBodyCell>
            <TableBodyCell>
              <span class="font-medium">Games:</span>
              {#if $loadingGames}
                <Spinner size="4" />
              {:else}
                {$unfilteredGames.length}
              {/if}
            </TableBodyCell>
            <TableBodyCell>
              <span class="font-medium">Roms:</span>
              {#if $loadingRoms}
                <Spinner size="4" />
              {:else}
                {$unfilteredRoms.length}
              {/if}
            </TableBodyCell>
            <TableBodyCell>
              <span class="font-medium">Romfiles:</span>
              {#if $loadingRoms}
                <Spinner size="4" />
              {:else}
                {uniq($unfilteredRoms.filter((rom) => rom.romfile).map((rom) => rom.romfile.path)).length}
              {/if}
            </TableBodyCell>
          </TableBodyRow>
          <TableBodyRow>
            <TableBodyCell>
              <span class="font-medium">Total Original Size:</span>
              {#if $loadingSizes}
                <Spinner size="4" />
              {:else}
                {prettyBytes($totalOriginalSize)}
              {/if}
            </TableBodyCell>
            <TableBodyCell>
              <span class="font-medium">1G1R Original Size:</span>
              {#if $loadingSizes}
                <Spinner size="4" />
              {:else}
                {prettyBytes($oneRegionOriginalSize)}
              {/if}
            </TableBodyCell>
            <TableBodyCell>
              <span class="font-medium">Total Actual Size:</span>
              {#if $loadingSizes}
                <Spinner size="4" />
              {:else}
                {prettyBytes($totalActualSize)}
              {/if}
            </TableBodyCell>
            <TableBodyCell>
              <span class="font-medium">1G1R Actual Size:</span>
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
</div>
