<script>
  import { Button } from "flowbite-svelte";
  import { BellSolid } from "flowbite-svelte-icons";

  import { notifications } from "../store.js";

  let open = false;

  function toggle() {
    open = !open;
  }

  function clear() {
    notifications.set([]);
  }

  function formatTime(date) {
    return date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" });
  }

  const typeClasses = {
    info: "text-sky-400",
    success: "text-emerald-400",
    warning: "text-amber-400",
    error: "text-rose-400",
  };

  const typeDot = {
    info: "bg-sky-400",
    success: "bg-emerald-400",
    warning: "bg-amber-400",
    error: "bg-rose-400",
  };
</script>

<div class="relative">
  <Button color="dark" class="relative p-2.5" onclick={toggle} title="Notifications">
    <BellSolid />
    {#if $notifications.length > 0}
      <span
        class="absolute -top-1 -right-1 flex h-4 w-4 items-center justify-center rounded-full bg-rose-500 text-xs font-bold text-white"
      >
        {$notifications.length > 99 ? "99+" : $notifications.length}
      </span>
    {/if}
  </Button>

  {#if open}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="fixed inset-0 z-30" onclick={toggle} onkeydown={(e) => e.key === "Escape" && toggle()}></div>
    <div
      class="absolute top-full right-0 z-40 mt-1 flex w-80 flex-col rounded-lg border border-slate-600 bg-slate-800 shadow-xl"
    >
      <div class="flex items-center justify-between border-b border-slate-600 px-3 py-2">
        <span class="text-sm font-semibold text-slate-200">Notifications</span>
        {#if $notifications.length > 0}
          <button class="text-xs text-slate-400 hover:text-slate-200" onclick={clear}> Clear all </button>
        {/if}
      </div>
      <div class="max-h-80 overflow-y-auto">
        {#if $notifications.length === 0}
          <p class="px-3 py-4 text-center text-sm text-slate-400">No notifications</p>
        {:else}
          {#each $notifications as n (n.id)}
            <div class="flex items-start gap-2 border-b border-slate-700 px-3 py-2 last:border-0">
              <span class="mt-1.5 h-2 w-2 shrink-0 rounded-full {typeDot[n.type] ?? 'bg-slate-400'}"></span>
              <div class="min-w-0 flex-1">
                <p class="text-sm text-slate-200">{n.message}</p>
                <p class="text-xs text-slate-500">{formatTime(n.timestamp)}</p>
              </div>
            </div>
          {/each}
        {/if}
      </div>
    </div>
  {/if}
</div>
