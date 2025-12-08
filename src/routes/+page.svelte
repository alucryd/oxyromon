<script>
  import { uniq } from "lodash-es";
  import prettyBytes from "pretty-bytes";
  import { onMount } from "svelte";
  import { Card, List, Li, Tooltip, Button } from "flowbite-svelte";
  import {
    ChevronLeftOutline,
    ChevronRightOutline,
    ChevronDoubleLeftOutline,
    ChevronDoubleRightOutline,
  } from "flowbite-svelte-icons";

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
    systems,
    systemsPage,
    systemsTotalPages,
    totalActualSize,
    totalOriginalSize,
    unfilteredGames,
    unfilteredRoms,
    unfilteredSystems,
    wantedFilter,
  } from "../store.js";

  $: systemsFirstPage = $systemsPage == 1;
  $: systemsLastPage = $systemsPage == $systemsTotalPages;
  $: gamesFirstPage = $gamesPage == 1;
  $: gamesLastPage = $gamesPage == $gamesTotalPages;
  $: romsFirstPage = $romsPage == 1;
  $: romsLastPage = $romsPage == $romsTotalPages;

  function computeSystemColor(system) {
    if (system.completion == 2) {
      return "bg-green-100 dark:bg-green-900 text-green-900 dark:text-green-100";
    }

    if (system.completion == 1) {
      return "bg-yellow-100 dark:bg-yellow-900 text-yellow-900 dark:text-yellow-100";
    }

    return "bg-red-100 dark:bg-red-900 text-red-900 dark:text-red-100";
  }

  function computeGameColor(game) {
    if (game.sorting == 2) {
      return "bg-gray-100 dark:bg-gray-700 text-gray-900 dark:text-gray-100";
    }

    if (game.completion == 2) {
      return "bg-green-100 dark:bg-green-900 text-green-900 dark:text-green-100";
    }

    if (game.completion == 1) {
      return "bg-yellow-100 dark:bg-yellow-900 text-yellow-900 dark:text-yellow-100";
    }

    return "bg-red-100 dark:bg-red-900 text-red-900 dark:text-red-100";
  }

  function computeRomColor(rom) {
    if (rom.ignored) {
      return "bg-gray-100 dark:bg-gray-700 text-gray-900 dark:text-gray-100";
    }

    if (rom.romfile) {
      return "bg-green-100 dark:bg-green-900 text-green-900 dark:text-green-100";
    }

    return "bg-red-100 dark:bg-red-900 text-red-900 dark:text-red-100";
  }

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
  <div class="mt-20 mb-4 grid grid-cols-1 gap-4 md:grid-cols-8">
    <div class="flex flex-col md:col-span-2">
      <Card class="flex max-w-none flex-1 flex-col text-center">
        <h5 class="m-2 text-xl font-bold tracking-tight text-gray-900 dark:text-white">Systems</h5>
        <List class="flex-1 divide-y divide-gray-200 dark:divide-gray-700">
          {#each $systems as system, i}
            <Li
              id="lgi-system-{i}"
              class="cursor-pointer truncate {system.id == $systemId
                ? 'bg-blue-500 text-white dark:bg-blue-600'
                : ''} {computeSystemColor(system)}"
              on:click={() => {
                systemId.set(system.id);
              }}
            >
              {system.name}
            </Li>
            {#if system.description && system.description != system.name}
              <Tooltip triggeredBy="#lgi-system-{i}" placement="bottom">{system.description}</Tooltip>
            {/if}
          {/each}
        </List>
        <div class="m-4 flex items-center justify-center gap-2">
          <Button size="sm" color="alternative" disabled={systemsFirstPage} on:click={() => systemsPage.set(1)}>
            <ChevronDoubleLeftOutline class="h-4 w-4" />
          </Button>
          <Button
            size="sm"
            color="alternative"
            disabled={systemsFirstPage}
            on:click={() => systemsPage.update((n) => n - 1)}
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
            on:click={() => systemsPage.update((n) => n + 1)}
          >
            <ChevronRightOutline class="h-4 w-4" />
          </Button>
          <Button
            size="sm"
            color="alternative"
            disabled={systemsLastPage}
            on:click={() => systemsPage.set($systemsTotalPages)}
          >
            <ChevronDoubleRightOutline class="h-4 w-4" />
          </Button>
        </div>
      </Card>
    </div>
    <div class="flex flex-col md:col-span-2">
      <Card class="flex max-w-none flex-1 flex-col text-center">
        <h5 class="m-2 text-xl font-bold tracking-tight text-gray-900 dark:text-white">Games</h5>
        <List class="flex-1 divide-y divide-gray-200 dark:divide-gray-700">
          {#each $games as game, i}
            <Li
              id="lgi-game-{i}"
              class="cursor-pointer truncate {game.id == $gameId
                ? 'bg-blue-500 text-white dark:bg-blue-600'
                : ''} {computeGameColor(game)} {game.sorting == 1 ? 'font-bold' : ''}"
              on:click={() => {
                gameId.set(game.id);
              }}
            >
              {game.name}
            </Li>
            {#if game.description && game.description != game.name}
              <Tooltip triggeredBy="#lgi-game-{i}" placement="bottom">{game.description}</Tooltip>
            {/if}
          {/each}
        </List>
        <div class="m-4 flex items-center justify-center gap-2">
          <Button size="sm" color="alternative" disabled={gamesFirstPage} on:click={() => gamesPage.set(1)}>
            <ChevronDoubleLeftOutline class="h-4 w-4" />
          </Button>
          <Button
            size="sm"
            color="alternative"
            disabled={gamesFirstPage}
            on:click={() => gamesPage.update((n) => n - 1)}
          >
            <ChevronLeftOutline class="h-4 w-4" />
          </Button>
          <span class="px-3 py-1 text-sm text-gray-700 dark:text-gray-300">
            {$gamesPage} / {$gamesTotalPages}
          </span>
          <Button
            size="sm"
            color="alternative"
            disabled={gamesLastPage}
            on:click={() => gamesPage.update((n) => n + 1)}
          >
            <ChevronRightOutline class="h-4 w-4" />
          </Button>
          <Button
            size="sm"
            color="alternative"
            disabled={gamesLastPage}
            on:click={() => gamesPage.set($gamesTotalPages)}
          >
            <ChevronDoubleRightOutline class="h-4 w-4" />
          </Button>
        </div>
      </Card>
    </div>
    <div class="flex flex-col md:col-span-4">
      <Card class="flex max-w-none flex-1 flex-col text-center">
        <h5 class="m-2 text-xl font-bold tracking-tight text-gray-900 dark:text-white">Roms</h5>
        <List class="flex-1 divide-y divide-gray-200 dark:divide-gray-700">
          {#each $roms as rom}
            <Li class="truncate {computeRomColor(rom)}">
              {rom.name}
            </Li>
          {/each}
        </List>
        <div class="m-4 flex items-center justify-center gap-2">
          <Button size="sm" color="alternative" disabled={romsFirstPage} on:click={() => romsPage.set(1)}>
            <ChevronDoubleLeftOutline class="h-4 w-4" />
          </Button>
          <Button size="sm" color="alternative" disabled={romsFirstPage} on:click={() => romsPage.update((n) => n - 1)}>
            <ChevronLeftOutline class="h-4 w-4" />
          </Button>
          <span class="px-3 py-1 text-sm text-gray-700 dark:text-gray-300">
            {$romsPage} / {$romsTotalPages}
          </span>
          <Button size="sm" color="alternative" disabled={romsLastPage} on:click={() => romsPage.update((n) => n + 1)}>
            <ChevronRightOutline class="h-4 w-4" />
          </Button>
          <Button size="sm" color="alternative" disabled={romsLastPage} on:click={() => romsPage.set($romsTotalPages)}>
            <ChevronDoubleRightOutline class="h-4 w-4" />
          </Button>
        </div>
      </Card>
    </div>
  </div>
  <div class="mb-4">
    <Card class="max-w-none text-center">
      <h5 class="m-2 text-xl font-bold tracking-tight text-gray-900 dark:text-white">Statistics</h5>
      <div class="grid grid-cols-1 gap-4 p-2 md:grid-cols-4">
        <div class="text-sm">
          <span class="font-medium">Systems:</span>
          {$unfilteredSystems.length}
        </div>
        <div class="text-sm">
          <span class="font-medium">Games:</span>
          {$unfilteredGames.length}
        </div>
        <div class="text-sm">
          <span class="font-medium">Roms:</span>
          {$unfilteredRoms.length}
        </div>
        <div class="text-sm">
          <span class="font-medium">Romfiles:</span>
          {uniq($unfilteredRoms.filter((rom) => rom.romfile).map((rom) => rom.romfile.path)).length}
        </div>
      </div>
      <div class="grid grid-cols-1 gap-4 p-2 md:grid-cols-4">
        <div class="text-sm">
          <span class="font-medium">Total Original Size:</span>
          {prettyBytes($totalOriginalSize)}
        </div>
        <div class="text-sm">
          <span class="font-medium">1G1R Original Size:</span>
          {prettyBytes($oneRegionOriginalSize)}
        </div>
        <div class="text-sm">
          <span class="font-medium">Total Actual Size:</span>
          {prettyBytes($totalActualSize)}
        </div>
        <div class="text-sm">
          <span class="font-medium">1G1R Actual Size:</span>
          {prettyBytes($oneRegionActualSize)}
        </div>
      </div>
    </Card>
  </div>
</div>
