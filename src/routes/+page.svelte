<script>
  import { uniq } from "lodash-es";
  import prettyBytes from "pretty-bytes";
  import { onMount } from "svelte";
  import {
    Card,
    CardBody,
    CardFooter,
    CardHeader,
    CardTitle,
    Col,
    ListGroup,
    ListGroupItem,
    Pagination,
    PaginationItem,
    PaginationLink,
    Row,
    Tooltip,
  } from "sveltestrap";

  import {
    getGamesBySystemId,
    getRomsByGameIdAndSystemId,
    getSizesBySystemId,
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
  } from "../store.js";

  $: systemsFirstPage = $systemsPage == 1;
  $: systemsLastPage = $systemsPage == $systemsTotalPages;
  $: gamesFirstPage = $gamesPage == 1;
  $: gamesLastPage = $gamesPage == $gamesTotalPages;
  $: romsFirstPage = $romsPage == 1;
  $: romsLastPage = $romsPage == $romsTotalPages;

  function computeSystemColor(system) {
    return system.complete ? "list-group-item-success" : "list-group-item-danger";
  }

  function computeGameColor(game) {
    if (game.complete) {
      return "list-group-item-success";
    }

    if (game.sorting == 2) {
      return "list-group-item-secondary";
    }

    return "list-group-item-danger";
  }

  function computeRomColor(rom) {
    if (rom.romfile) {
      return "list-group-item-success";
    }

    if (rom.ignored) {
      return "list-group-item-secondary";
    }

    return "list-group-item-danger";
  }

  onMount(async () => {
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
    oneRegionFilter.subscribe(async () => {
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
    ignoredFilter.subscribe(async () => {
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

<Row class="mb-3">
  <Col sm="3" class="d-flex flex-column">
    <Card class="text-center flex-fill">
      <CardHeader>
        <CardTitle class="fs-5 mb-0">Systems</CardTitle>
      </CardHeader>
      <CardBody class="p-0">
        <ListGroup flush>
          {#each $systems as system, i}
            <ListGroupItem
              id="lgi-system-{i}"
              tag="button"
              action
              class="text-truncate {system.id == $systemId ? 'active' : ''} {computeSystemColor(system)}"
              on:click={() => {
                systemId.set(system.id);
              }}
            >
              {system.name}
            </ListGroupItem>
            {#if system.description && system.description != system.name}
              <Tooltip target="lgi-system-{i}" placement="bottom">{system.description}</Tooltip>
            {/if}
          {/each}
        </ListGroup>
      </CardBody>
      <CardFooter class="d-flex">
        <Pagination arialabel="Systems navigation" class="mx-auto" listClassName="mb-0">
          <PaginationItem bind:disabled={systemsFirstPage}>
            <PaginationLink first href="#" on:click={() => systemsPage.set(1)} />
          </PaginationItem>
          <PaginationItem bind:disabled={systemsFirstPage}>
            <PaginationLink previous href="#" on:click={() => systemsPage.update((n) => n - 1)} />
          </PaginationItem>
          <PaginationItem disabled>
            <PaginationLink href="#">
              {$systemsPage} / {$systemsTotalPages}
            </PaginationLink>
          </PaginationItem>
          <PaginationItem bind:disabled={systemsLastPage}>
            <PaginationLink next href="#" on:click={() => systemsPage.update((n) => n + 1)} />
          </PaginationItem>
          <PaginationItem bind:disabled={systemsLastPage}>
            <PaginationLink last href="#" on:click={() => systemsPage.set($systemsTotalPages)} />
          </PaginationItem>
        </Pagination>
      </CardFooter>
    </Card>
  </Col>
  <Col sm="3" class="d-flex flex-column">
    <Card class="text-center flex-fill">
      <CardHeader>
        <CardTitle class="fs-5 mb-0">Games</CardTitle>
      </CardHeader>
      <CardBody class="p-0">
        <ListGroup flush>
          {#each $games as game, i}
            <ListGroupItem
              id="lgi-game-{i}"
              tag="button"
              action
              class="text-truncate {game.id == $gameId ? 'active' : ''} {computeGameColor(game)} {game.sorting == 1
                ? 'fw-bold'
                : ''}"
              on:click={() => {
                gameId.set(game.id);
              }}
            >
              {game.name}
            </ListGroupItem>
            {#if game.description && game.description != game.name}
              <Tooltip target="lgi-game-{i}" placement="bottom">{game.description}</Tooltip>
            {/if}
          {/each}
        </ListGroup>
      </CardBody>
      <CardFooter class="d-flex">
        <Pagination arialabel="Games navigation" class="mx-auto" listClassName="mb-0">
          <PaginationItem bind:disabled={gamesFirstPage}>
            <PaginationLink first href="#" on:click={() => gamesPage.set(1)} />
          </PaginationItem>
          <PaginationItem bind:disabled={gamesFirstPage}>
            <PaginationLink previous href="#" on:click={() => gamesPage.update((n) => n - 1)} />
          </PaginationItem>
          <PaginationItem disabled>
            <PaginationLink href="#">
              {$gamesPage} / {$gamesTotalPages}
            </PaginationLink>
          </PaginationItem>
          <PaginationItem bind:disabled={gamesLastPage}>
            <PaginationLink next href="#" on:click={() => gamesPage.update((n) => n + 1)} />
          </PaginationItem>
          <PaginationItem bind:disabled={gamesLastPage}>
            <PaginationLink last href="#" on:click={() => gamesPage.set($gamesTotalPages)} />
          </PaginationItem>
        </Pagination>
      </CardFooter>
    </Card>
  </Col>
  <Col sm="6" class="d-flex flex-column">
    <Card class="text-center flex-fill">
      <CardHeader>
        <CardTitle class="fs-5 mb-0">Roms</CardTitle>
      </CardHeader>
      <CardBody class="p-0">
        <ListGroup flush>
          {#each $roms as rom}
            <ListGroupItem class="text-truncate {computeRomColor(rom)}">
              {rom.name}
            </ListGroupItem>
          {/each}
        </ListGroup>
      </CardBody>
      <CardFooter class="d-flex">
        <Pagination arialabel="Roms navigation" class="mx-auto" listClassName="mb-0">
          <PaginationItem bind:disabled={romsFirstPage}>
            <PaginationLink first href="#" on:click={() => romsPage.set(1)} />
          </PaginationItem>
          <PaginationItem bind:disabled={romsFirstPage}>
            <PaginationLink previous href="#" on:click={() => romsPage.update((n) => n - 1)} />
          </PaginationItem>
          <PaginationItem disabled>
            <PaginationLink href="#">
              {$romsPage} / {$romsTotalPages}
            </PaginationLink>
          </PaginationItem>
          <PaginationItem bind:disabled={romsLastPage}>
            <PaginationLink next href="#" on:click={() => romsPage.update((n) => n + 1)} />
          </PaginationItem>
          <PaginationItem bind:disabled={romsLastPage}>
            <PaginationLink last href="#" on:click={() => romsPage.set($romsTotalPages)} />
          </PaginationItem>
        </Pagination>
      </CardFooter>
    </Card>
  </Col>
</Row>
<Row class="mb-3">
  <Col>
    <Card class="text-center">
      <CardHeader>
        <CardTitle class="fs-5 mb-0">Statistics</CardTitle>
      </CardHeader>
      <CardBody class="p-0" />
      <Row class="p-1">
        <Col>
          Systems: {$unfilteredSystems.length}
        </Col>
        <Col>
          Games: {$unfilteredGames.length}
        </Col>
        <Col>
          Roms: {$unfilteredRoms.length}
        </Col>
        <Col>
          Romfiles: {uniq($unfilteredRoms.filter((rom) => rom.romfile).map((rom) => rom.romfile.path)).length}
        </Col>
      </Row>
      <Row class="p-1">
        <Col>
          Total Original Size: {prettyBytes($totalOriginalSize)}
        </Col>
        <Col>
          1G1R Original Size: {prettyBytes($oneRegionOriginalSize)}
        </Col>
        <Col>
          Total Actual Size: {prettyBytes($totalActualSize)}
        </Col>
        <Col>
          1G1R Actual Size: {prettyBytes($oneRegionActualSize)}
        </Col>
      </Row>
    </Card>
  </Col>
</Row>
