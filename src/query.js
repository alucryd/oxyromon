import { GraphQLClient, gql } from "graphql-request";
import { reject } from "lodash-es";
import { get } from "svelte/store";

import {
  allRegions,
  allRegionsKey,
  allRegionsSubfolders,
  allRegionsSubfoldersKey,
  completeFilter,
  discardFlags,
  discardFlagsKey,
  discardReleases,
  discardReleasesKey,
  filteredGames,
  games,
  gamesPage,
  gamesTotalPages,
  groupSubsystems,
  groupSubsystemsKey,
  ignoredFilter,
  incompleteFilter,
  languages,
  languagesKey,
  nameFilter,
  oneRegionActualSize,
  oneRegionFilter,
  oneRegionOriginalSize,
  oneRegions,
  oneRegionsKey,
  oneRegionsSubfolders,
  oneRegionsSubfoldersKey,
  pageSize,
  preferFlags,
  preferFlagsKey,
  preferParents,
  preferParentsKey,
  preferRegions,
  preferRegionsKey,
  preferVersions,
  preferVersionsKey,
  romDirectory,
  romDirectoryKey,
  roms,
  romsPage,
  romsTotalPages,
  strictOneRegions,
  strictOneRegionsKey,
  systems,
  systemsPage,
  systemsTotalPages,
  tmpDirectory,
  tmpDirectoryKey,
  totalActualSize,
  totalOriginalSize,
  unfilteredGames,
  unfilteredRoms,
  unfilteredSystems,
  wantedFilter,
} from "./store.js";

// const endpoint = `${window.location.origin}/graphql`;
const endpoint = "http://localhost:8000/graphql";
const graphQLClient = new GraphQLClient(endpoint);

function paginate(array, page, pageSize) {
  const start = pageSize * (page - 1);
  const end = Math.min(pageSize * page, array.length);
  return array.slice(start, end);
}

function splitList(list) {
  return list ? list.split("|") : [];
}

export async function getSettings() {
  const query = gql`
    {
      settings {
        key
        value
      }
    }
  `;

  const data = await graphQLClient.request(query);

  oneRegions.set(splitList(data.settings.find((setting) => setting.key === oneRegionsKey).value));
  allRegions.set(splitList(data.settings.find((setting) => setting.key === allRegionsKey).value));
  languages.set(splitList(data.settings.find((setting) => setting.key === languagesKey).value));
  discardReleases.set(splitList(data.settings.find((setting) => setting.key === discardReleasesKey).value));
  discardFlags.set(splitList(data.settings.find((setting) => setting.key === discardFlagsKey).value));

  strictOneRegions.set(data.settings.find((setting) => setting.key === strictOneRegionsKey).value === "true");
  preferParents.set(data.settings.find((setting) => setting.key === preferParentsKey).value === "true");
  preferRegions.set(data.settings.find((setting) => setting.key === preferRegionsKey).value);
  preferVersions.set(data.settings.find((setting) => setting.key === preferVersionsKey).value);
  preferFlags.set(splitList(data.settings.find((setting) => setting.key === preferFlagsKey).value));

  romDirectory.set(data.settings.find((setting) => setting.key === romDirectoryKey).value);
  tmpDirectory.set(data.settings.find((setting) => setting.key === tmpDirectoryKey).value);
  groupSubsystems.set(data.settings.find((setting) => setting.key === groupSubsystemsKey).value === "true");
  oneRegionsSubfolders.set(data.settings.find((setting) => setting.key === oneRegionsSubfoldersKey).value);
  allRegionsSubfolders.set(data.settings.find((setting) => setting.key === allRegionsSubfoldersKey).value);
}

export async function getSystems() {
  const query = gql`
    {
      systems {
        id
        name
        description
        completion
        merging
        arcade
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
                completion
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
    games = reject(games, (game) => game.completion == 2);
  }
  if (!get(incompleteFilter)) {
    games = reject(games, (game) => game.completion == 1);
  }
  if (!get(wantedFilter)) {
    games = reject(games, (game) => game.completion == 0);
  }
  if (!get(ignoredFilter)) {
    games = reject(games, (game) => game.sorting === 2);
  }
  if (get(oneRegionFilter)) {
    games = reject(games, (game) => game.sorting !== 1);
  }
  if (get(nameFilter).length) {
    games = reject(
      games,
      (game) => !game.name.normalize("NFC").toLowerCase().includes(get(nameFilter).normalize("NFC").toLocaleLowerCase())
    );
  }
  return games;
}

export async function updateGames() {
  filteredGames.set(filterGames(get(unfilteredGames)));
  gamesTotalPages.set(Math.max(Math.ceil(get(filteredGames).length / get(pageSize)), 1));
  games.set(paginate(get(filteredGames), get(gamesPage), get(pageSize)));
}

export async function getRomsByGameIdAndSystemId(gameId, systemId) {
  const query = gql`
        {
            roms(gameId: ${gameId}) {
                name
                size
                romfile {
                    path
                    size
                }
                ignored(systemId: ${systemId})
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
