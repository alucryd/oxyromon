<script>
    import { onMount } from "svelte";
    import {
        Navbar,
        NavbarBrand,
        NavbarToggler,
        Collapse,
        Container,
        Row,
        Col,
        Card,
        CardHeader,
        CardTitle,
        CardBody,
        CardFooter,
        ListGroup,
        ListGroupItem,
        Pagination,
        PaginationItem,
        PaginationLink,
        InputGroup,
        Input,
        ButtonToolbar,
        ButtonGroup,
        Button,
    } from "sveltestrap";

    import {
        systems,
        unfilteredSystems,
        systemsPage,
        systemsTotalPages,
        games,
        unfilteredGames,
        gamesPage,
        gamesTotalPages,
        roms,
        unfilteredRoms,
        romsPage,
        romsTotalPages,
        pageSize,
        systemId,
        gameId,
        completeFilter,
        incompleteFilter,
        ignoredFilter,
        oneRegionFilter,
        nameFilter,
        totalOriginalSize,
        oneRegionOriginalSize,
        totalActualSize,
        oneRegionActualSize,
    } from "./stores.js";
    import {
        getSystems,
        updateSystems,
        getGamesBySystemId,
        updateGames,
        getRomsByGameId,
        updateRoms,
        getSizesBySystemId,
    } from "./mutations.js";

    import prettyBytes from "pretty-bytes";
    import { uniq } from "lodash-es";

    let navbarIsOpen = false;

    function handleNavbarUpdate(event) {
        navbarIsOpen = event.detail.isOpen;
    }

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
            if (game.sorting == "ONE_REGION") {
                return "list-group-item-primary";
            }
            return "list-group-item-success";
        } else {
            if (game.sorting == "IGNORED") {
                return "list-group-item-secondary";
            }
            return "list-group-item-danger";
        }
    }

    onMount(async () => {
        systemsPage.subscribe(async (_) => {
            await updateSystems();
        });
        systemId.subscribe(async (systemId) => {
            await getGamesBySystemId(systemId);
            await getSizesBySystemId(systemId);
        });
        gamesPage.subscribe(async (_) => {
            await updateGames();
        });
        gameId.subscribe(async (gameId) => {
            await getRomsByGameId(gameId);
        });
        romsPage.subscribe(async (_) => {
            await updateRoms();
        });
        pageSize.subscribe(async (_) => {
            await updateSystems();
            await updateGames();
            await updateRoms();
        });
        completeFilter.subscribe(async (_) => {
            if ($gamesPage != 1) {
                gamesPage.set(1);
            } else {
                await updateGames();
            }
        });
        oneRegionFilter.subscribe(async (_) => {
            if ($gamesPage != 1) {
                gamesPage.set(1);
            } else {
                await updateGames();
            }
        });
        incompleteFilter.subscribe(async (_) => {
            if ($gamesPage != 1) {
                gamesPage.set(1);
            } else {
                await updateGames();
            }
        });
        ignoredFilter.subscribe(async (_) => {
            if ($gamesPage != 1) {
                gamesPage.set(1);
            } else {
                await updateGames();
            }
        });
        nameFilter.subscribe(async (_) => {
            if ($gamesPage != 1) {
                gamesPage.set(1);
            } else {
                await updateGames();
            }
        });
        await getSystems();
    });
</script>

<main>
    <Navbar color="dark" dark sticky="top" expand="md" class="mb-3">
        <NavbarBrand href="/" class="ms-3">oxyromon</NavbarBrand>
        <NavbarToggler on:click={() => (navbarIsOpen = !navbarIsOpen)} />
        <Collapse {navbarIsOpen} navbar expand="md" class="d-flex justify-content-end" on:update={handleNavbarUpdate}>
            <ButtonToolbar class="d-flex">
                <ButtonGroup>
                    <Button
                        color="success"
                        bind:active={$completeFilter}
                        on:click={() => completeFilter.update((b) => !b)}
                    >
                        Complete
                    </Button>
                    <Button
                        color="primary"
                        bind:active={$oneRegionFilter}
                        on:click={() => oneRegionFilter.update((b) => !b)}
                    >
                        1G1R
                    </Button>
                    <Button
                        color="danger"
                        bind:active={$incompleteFilter}
                        on:click={() => incompleteFilter.update((b) => !b)}
                    >
                        Incomplete
                    </Button>
                    <Button
                        color="secondary"
                        bind:active={$ignoredFilter}
                        on:click={() => ignoredFilter.update((b) => !b)}
                    >
                        Ignored
                    </Button>
                </ButtonGroup>
                <InputGroup class="ms-3">
                    <Input placeholder="Game Name" bind:value={$nameFilter} />
                </InputGroup>
            </ButtonToolbar>
        </Collapse> />
    </Navbar>
    <Container fluid="true">
        <div class="card-group" />
        <Row class="mb-3">
            <Col sm="3" class="d-flex flex-column">
                <Card class="text-center flex-fill">
                    <CardHeader>
                        <CardTitle class="fs-5 mb-0">Systems</CardTitle>
                    </CardHeader>
                    <CardBody class="p-0">
                        <ListGroup flush>
                            {#each $systems as system}
                                <ListGroupItem
                                    tag="button"
                                    action
                                    class="text-truncate {system.id == $systemId ? 'active' : ''} {computeSystemColor(
                                        system
                                    )}"
                                    on:click={() => {
                                        systemId.set(system.id);
                                    }}
                                >
                                    {system.name}
                                </ListGroupItem>
                            {/each}
                        </ListGroup>
                    </CardBody>
                    <CardFooter class="d-flex">
                        <Pagination ariaLabel="Systems navigation" class="mx-auto" listClassName="mb-0">
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
                            {#each $games as game}
                                <ListGroupItem
                                    tag="button"
                                    action
                                    class="text-truncate {game.id == $gameId ? 'active' : ''} {computeGameColor(game)}"
                                    on:click={() => {
                                        gameId.set(game.id);
                                    }}
                                >
                                    {game.name}
                                </ListGroupItem>
                            {/each}
                        </ListGroup>
                    </CardBody>
                    <CardFooter class="d-flex">
                        <Pagination ariaLabel="Games navigation" class="mx-auto" listClassName="mb-0">
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
                                <ListGroupItem class="text-truncate">
                                    <span>
                                        {rom.name}
                                    </span>
                                </ListGroupItem>
                            {/each}
                        </ListGroup>
                    </CardBody>
                    <CardFooter class="d-flex">
                        <Pagination ariaLabel="Roms navigation" class="mx-auto" listClassName="mb-0">
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
                                <PaginationLink last href="#" on:click={romsPage.set($romsTotalPages)} />
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
                            Romfiles: {uniq($unfilteredRoms.filter((rom) => rom.romfile).map((rom) => rom.romfile.path))
                                .length}
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
    </Container>
</main>

<style>
    @import "https://cdn.jsdelivr.net/npm/bootstrap@5.0.0-beta3/dist/css/bootstrap.min.css";
</style>
