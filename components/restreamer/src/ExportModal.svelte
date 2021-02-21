<script lang="js">
  import { onDestroy } from 'svelte';
  import { mutation } from 'svelte-apollo';

  import { AddOutput } from './api/graphql/client.graphql';

  import { exportModal as value } from './stores';

  import { showError } from './util';

  const addOutputMutation = mutation(AddOutput);

  export let full_state;

  let submitable = false;
  onDestroy(
    value.subscribe((v) => {
      submitable = v.spec != v.prev_spec;
    })
  );

  let invalidSpec = null;
  onDestroy(value.subscribe((v) => validateSpec(v.spec)));
  function validateSpec(v) {
    try {
      JSON.parse(v);
      invalidSpec = null;
    } catch (e) {
      invalidSpec = 'Failed to parse JSON: ' + e.message;
    }
  }

  function onAreaClick(event) {
    if (event.target.classList.contains('uk-modal')) {
      value.close();
    }
  }

  function submit() {
    // TODO
  }
</script>

<template>
  <div class="uk-modal" class:uk-open={$value.visible} on:click={onAreaClick}>
    <div class="uk-modal-dialog uk-modal-body">
      <h2 class="uk-modal-title">Export or import as JSON</h2>
      <button
        class="uk-modal-close-outside"
        uk-close
        type="button"
        on:click={() => value.close()}
      />

      <fieldset>
        <textarea
          class="uk-textarea"
          class:uk-form-danger={!!invalidSpec}
          bind:value={$value.spec}
          on:change={() => validateSpec($value.spec)}
          placeholder="JSON..."
        />
        {#if !!invalidSpec}
          <span class="uk-form-danger spec-err">{invalidSpec}</span>
        {/if}
      </fieldset>

      <button
        class="uk-button uk-button-primary"
        disabled={!submitable}
        on:click={submit}>Apply</button
      >
    </div>
  </div>
</template>

<style lang="stylus">
  .uk-modal
    &.uk-open
      display: block

    .uk-modal-title
      font-size: 1.5rem

    fieldset
      border: none

      .uk-textarea
        min-height: 200px
        resize: none

      .spec-err
        display: block
        font-size: 11px
</style>
