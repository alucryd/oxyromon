import { GraphQLClient, gql } from "graphql-request";
import { systems, games, roms } from "./stores.js";

const endpoint = "http://localhost:8000/graphql";
const graphQLClient = new GraphQLClient(endpoint);

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
}
