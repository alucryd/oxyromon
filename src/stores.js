import { writable } from "svelte/store";

export const systems = writable([]);
export const systemId = writable(-1);
export const games = writable([]);
export const gameId = writable(-1);
export const roms = writable([]);

export const unfilteredSystems = writable([]);
export const unfilteredGames = writable([]);
export const filteredGames = writable([]);
export const unfilteredRoms = writable([]);

export const totalOriginalSize = writable((0));
export const oneRegionOriginalSize = writable((0));
export const totalActualSize = writable((0));
export const oneRegionActualSize = writable((0));

export const systemsPage = writable(1);
export const systemsTotalPages = writable(1);
export const gamesPage = writable(1);
export const gamesTotalPages = writable(1);
export const romsPage = writable(1);
export const romsTotalPages = writable(1);
export const pageSize = writable(20);

export const completeFilter = writable(true);
export const oneRegionFilter = writable(true);
export const incompleteFilter = writable(true);
export const ignoredFilter = writable(true);
export const nameFilter = writable("");
