<script>
  import { Modal, Badge, A } from "flowbite-svelte";
  import { MugHotSolid } from "flowbite-svelte-icons";

  import { getInfo } from "../query.js";
  import { isAboutModalOpen } from "../store.js";

  let info = null;

  $: if ($isAboutModalOpen && !info) {
    getInfo().then((data) => (info = data));
  }
</script>

<Modal title="About oxyROMon" bind:open={$isAboutModalOpen} size="md" class="text-start">
  <div class="space-y-4">
    <div class="flex flex-col items-center gap-2 pb-2">
      <div class="rounded-xl bg-slate-800 p-3">
        <img src="/logo.svg" alt="logo" style="height: 48px;" />
      </div>
      {#if info}
        <p class="text-lg font-semibold">oxyROMon {info.version}</p>
      {/if}
      <p class="text-sm text-gray-500 dark:text-gray-400">Rusty ROM OrgaNizer</p>
    </div>

    <h6 class="text-sm font-medium text-gray-500 uppercase dark:text-gray-400">Statistics</h6>
    {#if info}
      <div class="grid grid-cols-3 gap-2 text-center">
        <div class="rounded bg-gray-100 p-2 dark:bg-gray-700">
          <p class="text-lg font-bold">{info.systemCount}</p>
          <p class="text-xs text-gray-500 dark:text-gray-400">Systems</p>
        </div>
        <div class="rounded bg-gray-100 p-2 dark:bg-gray-700">
          <p class="text-lg font-bold">{info.gameCount}</p>
          <p class="text-xs text-gray-500 dark:text-gray-400">Games</p>
        </div>
        <div class="rounded bg-gray-100 p-2 dark:bg-gray-700">
          <p class="text-lg font-bold">{info.romCount}</p>
          <p class="text-xs text-gray-500 dark:text-gray-400">ROMs</p>
        </div>
      </div>
    {:else}
      <p class="text-sm text-gray-500 dark:text-gray-400">Loading...</p>
    {/if}

    <h6 class="text-sm font-medium text-gray-500 uppercase dark:text-gray-400">Dependencies</h6>
    {#if info}
      <div class="flex flex-wrap gap-2">
        {#each info.dependencies as dep (dep.name)}
          <Badge color={dep.version ? "green" : "red"} large>
            {dep.name}{dep.version && dep.version !== "unknown" ? ` ${dep.version}` : ""}
          </Badge>
        {/each}
      </div>
    {:else}
      <p class="text-sm text-gray-500 dark:text-gray-400">Loading...</p>
    {/if}

    <div class="border-t border-gray-200 pt-4 dark:border-gray-600">
      <p class="text-sm text-gray-500 dark:text-gray-400">
        If you find oxyROMon useful, please consider
        <A href="https://ko-fi.com/alucryd" target="_blank" class="inline-flex items-center gap-1">
          <MugHotSolid class="h-4 w-4" /> buying me a coffee
        </A>.
      </p>
    </div>
  </div>
</Modal>
