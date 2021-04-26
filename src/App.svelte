<script>
    import { onMount } from "svelte";
    import {
        systemsView,
        systemsPage,
        systemsTotalPages,
        gamesView,
        gamesPage,
        gamesTotalPages,
        romsView,
        romsPage,
        romsTotalPages,
        pageSize,
        systemId,
        gameId,
    } from "./stores.js";
    import {
        getSystems,
        updateSystemsView,
        getGamesBySystemId,
        updateGamesView,
        getRomsByGameId,
        updateRomsView,
    } from "./mutations.js";

    import {
        Navbar,
        NavbarBrand,
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
    } from "sveltestrap";

    $: systemsFirstPage = $systemsPage == 1;
    $: systemsLastPage = $systemsPage == $systemsTotalPages;
    $: gamesFirstPage = $gamesPage == 1;
    $: gamesLastPage = $gamesPage == $gamesTotalPages;
    $: romsFirstPage = $romsPage == 1;
    $: romsLastPage = $romsPage == $romsTotalPages;

    onMount(async () => {
        systemsPage.subscribe(async (_) => {
            await updateSystemsView();
        });
        systemId.subscribe(async (systemId) => {
            await getGamesBySystemId(systemId);
        });
        gamesPage.subscribe(async (_) => {
            await updateGamesView();
        });
        gameId.subscribe(async (gameId) => {
            await getRomsByGameId(gameId);
        });
        romsPage.subscribe(async (_) => {
            await updateRomsView();
        });
        pageSize.subscribe(async (_) => {
            await updateSystemsView();
            await updateGamesView();
            await updateRomsView();
        });
        await getSystems();
    });
</script>

<main>
    <Navbar color="dark" dark sticky="top" class="mb-3">
        <NavbarBrand href="/" class="ms-3">oxyromon</NavbarBrand>
    </Navbar>
    <Container fluid="true">
        <Row>
            <Col sm="3">
                <Card class="text-center">
                    <CardHeader>
                        <CardTitle class="fs-5 mb-0">Systems</CardTitle>
                    </CardHeader>
                    <CardBody class="p-0">
                        <ListGroup flush>
                            {#each $systemsView as system}
                                <ListGroupItem
                                    tag="button"
                                    action
                                    class="text-truncate {system.id == $systemId
                                        ? 'active'
                                        : ''}"
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
                        <Pagination
                            ariaLabel="Systems navigation"
                            class="mx-auto"
                            listClassName="mb-0"
                        >
                            <PaginationItem bind:disabled={systemsFirstPage}>
                                <PaginationLink
                                    first
                                    href="#"
                                    on:click={() => systemsPage.set(1)}
                                />
                            </PaginationItem>
                            <PaginationItem bind:disabled={systemsFirstPage}>
                                <PaginationLink
                                    previous
                                    href="#"
                                    on:click={() =>
                                        systemsPage.update((n) => n - 1)}
                                />
                            </PaginationItem>
                            <PaginationItem disabled>
                                <PaginationLink href="#">
                                    {$systemsPage} / {$systemsTotalPages}
                                </PaginationLink>
                            </PaginationItem>
                            <PaginationItem bind:disabled={systemsLastPage}>
                                <PaginationLink
                                    next
                                    href="#"
                                    on:click={() =>
                                        systemsPage.update((n) => n + 1)}
                                />
                            </PaginationItem>
                            <PaginationItem bind:disabled={systemsLastPage}>
                                <PaginationLink
                                    last
                                    href="#"
                                    on:click={() =>
                                        systemsPage.set($systemsTotalPages)}
                                />
                            </PaginationItem>
                        </Pagination>
                    </CardFooter>
                </Card>
            </Col>
            <Col sm="3">
                <Card class="text-center">
                    <CardHeader>
                        <CardTitle class="fs-5 mb-0">Games</CardTitle>
                    </CardHeader>
                    <CardBody class="p-0">
                        <ListGroup flush>
                            {#each $gamesView as game}
                                <ListGroupItem
                                    tag="button"
                                    action
                                    class="text-truncate {game.id == $gameId
                                        ? 'active'
                                        : ''}"
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
                        <Pagination
                            ariaLabel="Games navigation"
                            class="mx-auto"
                            listClassName="mb-0"
                        >
                            <PaginationItem bind:disabled={gamesFirstPage}>
                                <PaginationLink
                                    first
                                    href="#"
                                    on:click={() => gamesPage.set(1)}
                                />
                            </PaginationItem>
                            <PaginationItem bind:disabled={gamesFirstPage}>
                                <PaginationLink
                                    previous
                                    href="#"
                                    on:click={() =>
                                        gamesPage.update((n) => n - 1)}
                                />
                            </PaginationItem>
                            <PaginationItem disabled>
                                <PaginationLink href="#">
                                    {$gamesPage} / {$gamesTotalPages}
                                </PaginationLink>
                            </PaginationItem>
                            <PaginationItem bind:disabled={gamesLastPage}>
                                <PaginationLink
                                    next
                                    href="#"
                                    on:click={() =>
                                        gamesPage.update((n) => n + 1)}
                                />
                            </PaginationItem>
                            <PaginationItem bind:disabled={gamesLastPage}>
                                <PaginationLink
                                    last
                                    href="#"
                                    on:click={() =>
                                        gamesPage.set($gamesTotalPages)}
                                />
                            </PaginationItem>
                        </Pagination>
                    </CardFooter>
                </Card>
            </Col>
            <Col sm="6">
                <Card class="text-center">
                    <CardHeader>
                        <CardTitle class="fs-5 mb-0">Roms</CardTitle>
                    </CardHeader>
                    <CardBody class="p-0">
                        <ListGroup flush>
                            {#each $romsView as rom}
                                <ListGroupItem class="text-truncate">
                                    <span>
                                        {rom.name}
                                    </span>
                                </ListGroupItem>
                            {/each}
                        </ListGroup>
                    </CardBody>
                    <CardFooter class="d-flex">
                        <Pagination
                            ariaLabel="Roms navigation"
                            class="mx-auto"
                            listClassName="mb-0"
                        >
                            <PaginationItem bind:disabled={romsFirstPage}>
                                <PaginationLink
                                    first
                                    href="#"
                                    on:click={() => romsPage.set(1)}
                                />
                            </PaginationItem>
                            <PaginationItem bind:disabled={romsFirstPage}>
                                <PaginationLink
                                    previous
                                    href="#"
                                    on:click={() =>
                                        romsPage.update((n) => n - 1)}
                                />
                            </PaginationItem>
                            <PaginationItem disabled>
                                <PaginationLink href="#">
                                    {$romsPage} / {$romsTotalPages}
                                </PaginationLink>
                            </PaginationItem>
                            <PaginationItem bind:disabled={romsLastPage}>
                                <PaginationLink
                                    next
                                    href="#"
                                    on:click={() =>
                                        romsPage.update((n) => n + 1)}
                                />
                            </PaginationItem>
                            <PaginationItem bind:disabled={romsLastPage}>
                                <PaginationLink
                                    last
                                    href="#"
                                    on:click={romsPage.set($romsTotalPages)}
                                />
                            </PaginationItem>
                        </Pagination>
                    </CardFooter>
                </Card>
            </Col>
        </Row>
    </Container>
</main>

<style>
    @import "https://cdn.jsdelivr.net/npm/bootstrap@5.0.0-beta3/dist/css/bootstrap.min.css";
</style>
