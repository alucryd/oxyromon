import { writable } from "svelte/store";

export const oneRegionsKey = "REGIONS_ONE";
export const allRegionsKey = "REGIONS_ALL";
export const languagesKey = "LANGUAGES";
export const discardReleasesKey = "DISCARD_RELEASES";
export const discardFlagsKey = "DISCARD_FLAGS";

export const strictOneRegionsKey = "REGIONS_ONE_STRICT";
export const preferParentsKey = "PREFER_PARENTS";
export const preferRegionsKey = "PREFER_REGIONS";
export const preferVersionsKey = "PREFER_VERSIONS";
export const preferFlagsKey = "PREFER_FLAGS";

export const romDirectoryKey = "ROM_DIRECTORY";
export const tmpDirectoryKey = "TMP_DIRECTORY";
export const groupSubsystemsKey = "GROUP_SUBSYSTEMS";
export const oneRegionsSubfoldersKey = "REGIONS_ONE_SUBFOLDERS";
export const allRegionsSubfoldersKey = "REGIONS_ALL_SUBFOLDERS";

export const preferRegionsChoices = ["none", "broad", "narrow"];
export const preferVersionsChoices = ["none", "new", "old"];
export const subfolderSchemesChoices = ["none", "alpha"];

export const oneRegions = writable([]);
export const allRegions = writable([]);
export const languages = writable([]);
export const discardReleases = writable([]);
export const discardFlags = writable([]);

export const strictOneRegions = writable(false);
export const preferParents = writable(true);
export const preferRegions = writable("none");
export const preferVersions = writable("none");
export const preferFlags = writable([]);

export const romDirectory = writable("");
export const tmpDirectory = writable("");
export const groupSubsystems = writable(true);
export const oneRegionsSubfolders = writable("none");
export const allRegionsSubfolders = writable("none");

export const systems = writable([]);
export const systemId = writable(-1);
export const purgingSystemId = writable(-1);
export const games = writable([]);
export const gameId = writable(-1);
export const roms = writable([]);
export const romfiles = writable([]);

export const unfilteredSystems = writable([]);
export const unfilteredGames = writable([]);
export const filteredGames = writable([]);
export const unfilteredRoms = writable([]);

export const totalOriginalSize = writable(0);
export const oneRegionOriginalSize = writable(0);
export const totalActualSize = writable(0);
export const oneRegionActualSize = writable(0);

export const systemsPage = writable(1);
export const systemsTotalPages = writable(1);
export const gamesPage = writable(1);
export const gamesTotalPages = writable(1);
export const romsPage = writable(1);
export const romsTotalPages = writable(1);
export const romfilesPage = writable(1);
export const romfilesTotalPages = writable(1);
export const pageSize = writable(20);
export const romsPageSize = writable(8);

export const completeFilter = writable(true);
export const incompleteFilter = writable(true);
export const wantedFilter = writable(true);
export const ignoredFilter = writable(true);
export const oneRegionFilter = writable(false);
export const nameFilter = writable("");

export const isSettingsModalOpen = writable(false);

export const loadingSystems = writable(false);
export const loadingGames = writable(false);
export const loadingRoms = writable(false);
export const loadingSizes = writable(false);
