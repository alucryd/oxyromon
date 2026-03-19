<script>
  import { Modal, Button, Toggle, Helper } from "flowbite-svelte";
  import { UploadSolid } from "flowbite-svelte-icons";

  import { datsEndpoint } from "../events.js";
  import { isImportDatModalOpen } from "../store.js";

  let updateOnly = false;
  let isImporting = false;
  let selectedFile = null;
  let fileInput;

  function handleFileChange(event) {
    selectedFile = event.target.files[0] ?? null;
  }

  function handleFileDrop(event) {
    event.preventDefault();
    selectedFile = event.dataTransfer.files[0] ?? null;
  }

  async function handleImport() {
    if (!selectedFile) return;
    isImporting = true;
    const formData = new FormData();
    formData.append("file", selectedFile);
    formData.append("update", updateOnly.toString());
    await fetch(datsEndpoint, { method: "POST", body: formData });
    isImporting = false;
    selectedFile = null;
    fileInput.value = "";
    isImportDatModalOpen.set(false);
  }

  function handleClose() {
    selectedFile = null;
    if (fileInput) fileInput.value = "";
    updateOnly = false;
  }
</script>

<Modal title="Import DAT" bind:open={$isImportDatModalOpen} size="sm" class="text-start" on:close={handleClose}>
  <div class="space-y-4">
    <div
      class="flex cursor-pointer flex-col items-center justify-center gap-2 rounded-lg border-2 border-dashed border-gray-300 p-6 text-center hover:border-gray-400 dark:border-gray-600 dark:hover:border-gray-500"
      onclick={() => fileInput.click()}
      ondrop={handleFileDrop}
      ondragover={(e) => e.preventDefault()}
      role="button"
      tabindex="0"
      onkeydown={(e) => e.key === "Enter" && fileInput.click()}
    >
      <UploadSolid class="h-8 w-8 text-gray-400 dark:text-gray-500" />
      {#if selectedFile}
        <p class="text-sm font-medium">{selectedFile.name}</p>
        <p class="text-xs text-gray-500 dark:text-gray-400">{(selectedFile.size / 1024).toFixed(1)} KB</p>
      {:else}
        <p class="text-sm font-medium">Click or drop a file here</p>
        <p class="text-xs text-gray-500 dark:text-gray-400">Supported formats: .dat, .zip</p>
      {/if}
    </div>
    <input bind:this={fileInput} type="file" accept=".dat,.zip" class="hidden" onchange={handleFileChange} />

    <div class="flex flex-col gap-1">
      <Toggle bind:checked={updateOnly}>Update only</Toggle>
      <Helper class="ml-11">Only import DAT files for systems already in the database.</Helper>
    </div>

    <div class="flex gap-2 pt-2">
      <Button onclick={handleImport} disabled={!selectedFile || isImporting}>
        {#if isImporting}Importing…{:else}Import{/if}
      </Button>
      <Button color="alternative" onclick={() => isImportDatModalOpen.set(false)}>Cancel</Button>
    </div>
  </div>
</Modal>
