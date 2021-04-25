<script>
    import { onMount } from "svelte";
    import { systems, games, roms } from "./stores.js";
    import {
        getSystems,
        getGamesBySystemId,
        getRomsByGameId,
    } from "./mutations.js";

    import { Container, Row, Col, ListGroup, ListGroupItem } from "sveltestrap";

    onMount(async () => {
        await getSystems();
    });
</script>

<main>
    <Container fluid="true">
        <Row>
            <Col>
                <h5>Systems</h5>
                <ListGroup flush>
                    {#each $systems as system}
                        <ListGroupItem>
                            <span
                                on:click={async () => {
                                    getGamesBySystemId(system.id);
                                }}
                            >
                                {system.name}
                            </span>
                        </ListGroupItem>
                    {/each}
                </ListGroup>
            </Col>
            <Col>
                <h5>Games</h5>
                <ListGroup flush>
                    {#each $games as game}
                        <ListGroupItem>
                            <span
                                on:click={async () => {
                                    getRomsByGameId(game.id);
                                }}
                            >
                                {game.name}
                            </span>
                        </ListGroupItem>
                    {/each}
                </ListGroup>
            </Col>
            <Col>
                <h5>Roms</h5>
                <ListGroup flush>
                    {#each $roms as rom}
                        <ListGroupItem>{rom.name}</ListGroupItem>
                    {/each}
                </ListGroup>
            </Col>
        </Row>
    </Container>
</main>

<style>
    @import "https://cdn.jsdelivr.net/npm/bootstrap@5.0.0-beta3/dist/css/bootstrap.min.css";
</style>
