import { get } from "svelte/store";
import { GraphQLClient, gql } from "graphql-request";
import {
    systems,
    unfilteredSystems,
    systemsPage,
    systemsTotalPages,
    games,
    unfilteredGames,
    filteredGames,
    gamesPage,
    gamesTotalPages,
    roms,
    unfilteredRoms,
    romsPage,
    romsTotalPages,
    pageSize,
    completeFilter,
    oneRegionFilter,
    incompleteFilter,
    ignoredFilter,
    nameFilter,
    totalOriginalSize,
    oneRegionOriginalSize,
    totalActualSize,
    oneRegionActualSize,
} from "./stores.js";

import { reject } from "lodash-es";

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
                complete
            }
        }
    `;

    const data = await graphQLClient.request(query);
    unfilteredSystems.set(data.systems);
    await updateSystems();
}

export async function updateSystems() {
    systemsTotalPages.set(Math.max(Math.ceil(get(unfilteredSystems).length / get(pageSize)), 1));
    systems.set(paginate(get(unfilteredSystems), get(systemsPage), get(pageSize)));
}

export async function getGamesBySystemId(systemId) {
    const query = gql`
        {
            games(systemId: ${systemId}) {
                id
                name
                description
                complete
                sorting
            }
        }
    `;

    const data = await graphQLClient.request(query);
    unfilteredGames.set(data.games);
    await updateGames();
}

function filterGames(games) {
    if (!get(completeFilter)) {
        games = reject(games, (game) => game.complete && game.sorting != "ONE_REGION");
    }
    if (!get(incompleteFilter)) {
        games = reject(games, (game) => !game.complete && game.sorting != "IGNORED");
    }
    if (!get(ignoredFilter)) {
        games = reject(games, (game) => game.sorting == "IGNORED");
    }
    if (!get(oneRegionFilter)) {
        games = reject(games, (game) => game.sorting == "ONE_REGION");
    }
    if (get(nameFilter).length) {
        games = reject(
            games,
            (game) =>
                !game.name.normalize("NFC").toLowerCase().includes(get(nameFilter).normalize("NFC").toLocaleLowerCase())
        );
    }
    return games;
}

export async function updateGames() {
    filteredGames.set(filterGames(get(unfilteredGames)));
    gamesTotalPages.set(Math.max(Math.ceil(get(filteredGames).length / get(pageSize)), 1));
    games.set(paginate(get(filteredGames), get(gamesPage), get(pageSize)));
}

export async function getRomsByGameId(gameId) {
    const query = gql`
        {
            roms(gameId: ${gameId}) {
                name
                size
                romfile {
                    path
                    size
                }
            }
        }
    `;

    const data = await graphQLClient.request(query);
    unfilteredRoms.set(data.roms);
    await updateRoms();
}

export async function updateRoms() {
    romsTotalPages.set(Math.max(Math.ceil(get(unfilteredRoms).length / get(pageSize)), 1));
    roms.set(paginate(get(unfilteredRoms), get(romsPage), get(pageSize)));
}

export async function getSizesBySystemId(systemId) {
    const query = gql`
        {
            totalOriginalSize(systemId: ${systemId})
            oneRegionOriginalSize(systemId: ${systemId})
            totalActualSize(systemId: ${systemId})
            oneRegionActualSize(systemId: ${systemId})
        }
    `;
    const data = await graphQLClient.request(query);
    totalOriginalSize.set(data.totalOriginalSize);
    oneRegionOriginalSize.set(data.oneRegionOriginalSize);
    totalActualSize.set(data.totalActualSize);
    oneRegionActualSize.set(data.oneRegionActualSize);
}
