import { writable } from 'svelte/store';

export const systems = writable([]);
export const systemId = writable(-1);
export const games = writable([]);
export const gameId = writable(-1);
export const roms = writable([]);

export const systemsView = writable([]);
export const systemsPage = writable(1);
export const systemsTotalPages = writable(1);
export const gamesView = writable([]);
export const gamesPage = writable(1);
export const gamesTotalPages = writable(1);
export const romsView = writable([]);
export const romsPage = writable(1);
export const romsTotalPages = writable(1);
export const pageSize = writable(20);
