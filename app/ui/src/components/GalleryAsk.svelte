<script lang="ts">
  import { m } from '../paraglide/messages.js';
  import Dialog from './Dialog.svelte';
  import { gallery } from '../lib/stores/gallery.svelte';
</script>

<Dialog open={gallery.asking} title={m.gallery_ask_title()} describedby="gallery-ask-desc" onclose={() => gallery.reply(null)}>
  <p id="gallery-ask-desc">{m.gallery_ask_desc()}</p>
  {#snippet actions()}
    <button type="button" class="btn" data-testid="gallery-ask-files" onclick={() => gallery.reply(false)}>{m.gallery_ask_files()}</button>
    <!-- svelte-ignore a11y_autofocus -->
    <button type="button" class="btn primary" data-testid="gallery-ask-yes" autofocus onclick={() => gallery.reply(true)}>{m.gallery_ask_yes()}</button>
  {/snippet}
</Dialog>
