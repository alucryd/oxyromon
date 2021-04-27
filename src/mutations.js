import { get } from "svelte/store";
import { GraphQLClient, gql } from "graphql-request";
import { systems, systemsView, systemsPage, systemsTotalPages, games, gamesView, gamesPage, gamesTotalPages, roms, romsView, romsPage, romsTotalPages, pageSize } from "./stores.js";

const endpoint = "/graphql";
const graphQLClient = new GraphQLClient(endpoint);

function paginate(array, page, pageSize) {
    let start = pageSize * (page - 1);
    let end = Math.min(pageSize * page, array.length);
    return array.slice(start, end);
}

export async function getSystems() {
    const query = gql`
        {
            systems {
                id
                name
            }
        }
    `;

    const data = await graphQLClient.request(query);
    systems.set(data.systems);
    await updateSystemsView();
}

export async function updateSystemsView() {
    systemsTotalPages.set(Math.max(Math.ceil(get(systems).length / get(pageSize)), 1));
    systemsView.set(paginate(get(systems), get(systemsPage), get(pageSize)));
}

export async function getGamesBySystemId(systemId) {
    const query = gql`
        {
            games(systemId: ${systemId}) {
                id
                name
            }
        }
    `;

    const data = await graphQLClient.request(query);
    games.set(data.games);
    await updateGamesView();
}

export async function updateGamesView() {
    gamesTotalPages.set(Math.max(Math.ceil(get(games).length / get(pageSize)), 1));
    gamesView.set(paginate(get(games), get(gamesPage), get(pageSize)));
}

export async function getRomsByGameId(gameId) {
    const query = gql`
        {
            roms(gameId: ${gameId}) {
                name
            }
        }
    `;

    const data = await graphQLClient.request(query);
    roms.set(data.roms);
    await updateRomsView();
}

export async function updateRomsView() {
    romsTotalPages.set(Math.max(Math.ceil(get(roms).length / get(pageSize)), 1));
    romsView.set(paginate(get(roms), get(romsPage), get(pageSize)));
}
