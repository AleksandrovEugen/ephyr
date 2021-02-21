<svelte:options immutable={true} />

<script lang="js">
  import { mutation } from 'svelte-apollo';

  import {
    DisableInput,
    EnableInput,
    RemoveInput,
    DisableAllOutputs,
    EnableAllOutputs,
  } from './api/graphql/client.graphql';

  import { showError } from './util';

  import { inputModal, outputModal, exportModal } from './stores';

  import Output from './Output.svelte';
  import Toggle from './Toggle.svelte';

  const disableInputMutation = mutation(DisableInput);
  const enableInputMutation = mutation(EnableInput);
  const removeInputMutation = mutation(RemoveInput);
  const disableAllOutputsMutation = mutation(DisableAllOutputs);
  const enableAllOutputsMutation = mutation(EnableAllOutputs);

  export let public_host = 'localhost';
  export let value;

  $: isPull = value.input.__typename === 'PullInput';
  $: isFailover = value.input.__typename === 'FailoverPushInput';
  $: allEnabled = value.outputs.every((o) => o.enabled);

  $: mainStatus = isFailover ? value.input.mainStatus : value.input.status;

  $: onlineCount = value.outputs.filter((o) => o.status === 'ONLINE').length;
  $: initCount = value.outputs.filter((o) => o.status === 'INITIALIZING')
    .length;
  $: offlineCount = value.outputs.filter((o) => o.status === 'OFFLINE').length;
  $: presentBitmask =
    (onlineCount > 0 ? 1 : 0) +
    2 * (initCount > 0 ? 1 : 0) +
    4 * (offlineCount > 0 ? 1 : 0);

  let enabledBitmask = 0;
  $: if (enabledBitmask === presentBitmask) {
    enabledBitmask = 0;
  }

  $: showAll =
    (enabledBitmask & presentBitmask) === presentBitmask ||
    (enabledBitmask & presentBitmask) === 0;
  $: showFiltered = {
    ONLINE: !showAll && (enabledBitmask & 1) === 1,
    INITIALIZING: !showAll && (enabledBitmask & 2) === 2,
    OFFLINE: !showAll && (enabledBitmask & 4) === 4,
  };

  function openEditInputModal() {
    inputModal.openEdit(
      value.id,
      isPull ? value.input.src : value.input.name,
      value.label,
      isPull,
      isFailover
    );
  }

  async function removeInput() {
    try {
      await removeInputMutation({ variables: { id: value.id } });
    } catch (e) {
      showError(e.message);
    }
  }

  async function toggleInput() {
    const vars = { variables: { id: value.id } };
    try {
      if (value.enabled) {
        await disableInputMutation(vars);
      } else {
        await enableInputMutation(vars);
      }
    } catch (e) {
      showError(e.message);
    }
  }

  function openAddOutputModal() {
    outputModal.open(value.id);
  }

  async function toggleAllOutputs() {
    if (value.outputs.length < 1) return;
    try {
      if (allEnabled) {
        await disableAllOutputsMutation({ variables: { input_id: value.id } });
      } else {
        await enableAllOutputsMutation({ variables: { input_id: value.id } });
      }
    } catch (e) {
      showError(e.message);
    }
  }
</script>

<template>
  <div class="uk-section uk-section-muted uk-section-xsmall">
    <button type="button" class="uk-close" uk-close on:click={removeInput} />

    <button
      class="uk-button uk-button-primary uk-button-small"
      on:click={openAddOutputModal}
    >
      <i class="fas fa-plus" />&nbsp;<span>Output</span>
    </button>

    <a
      class="export-import"
      href="/"
      on:click|preventDefault={() => exportModal.open(value.id, value)}
      title="Export/Import"
    >
      <i class="fas fa-share-square" />
    </a>

    {#if !!value.label}
      <span class="label">{value.label}</span>
    {/if}

    {#if value.outputs && value.outputs.length > 0}
      <span class="total">
        {#if offlineCount > 0}
          <a
            href="/"
            on:click|preventDefault={() => (enabledBitmask ^= 4)}
            class:enabled={showFiltered['OFFLINE']}
            class="count uk-alert-danger">{offlineCount}</a
          >
        {/if}
        {#if initCount > 0}
          <a
            href="/"
            on:click|preventDefault={() => (enabledBitmask ^= 2)}
            class:enabled={showFiltered['INITIALIZING']}
            class="count uk-alert-warning">{initCount}</a
          >
        {/if}
        {#if onlineCount > 0}
          <a
            href="/"
            on:click|preventDefault={() => (enabledBitmask ^= 1)}
            class:enabled={showFiltered['ONLINE']}
            class="count uk-alert-success">{onlineCount}</a
          >
        {/if}
        <Toggle
          id="all-outputs-toggle-{value.id}"
          checked={allEnabled}
          title="{allEnabled ? 'Disable' : 'Enable'} all outputs"
          on:change={toggleAllOutputs}
        />
      </span>
    {/if}

    <Toggle
      id="input-toggle-{value.id}"
      checked={value.enabled}
      on:change={toggleInput}
    />
    <span>
      <span
        class:uk-alert-danger={mainStatus === 'OFFLINE'}
        class:uk-alert-warning={mainStatus === 'INITIALIZING'}
        class:uk-alert-success={mainStatus === 'ONLINE'}
      >
        {#key isPull}
          <span>
            <i
              class="fas"
              class:fa-arrow-down={isPull}
              class:fa-arrow-right={!isPull}
              title="{isPull ? 'Pulls' : 'Accepts'}{isFailover
                ? ' main'
                : ''} RTMP stream"
            />
          </span>
        {/key}
      </span>
      <span>
        {#if isPull}
          {value.input.src}
        {:else}
          rtmp://{public_host}/{value.input.name}/{isFailover ? 'main' : 'in'}
        {/if}
      </span>
      <a
        class="edit-input"
        href="/"
        on:click|preventDefault={openEditInputModal}
      >
        <i class="far fa-edit" title="Edit input" />
      </a>
    </span>
    {#if isFailover}
      <div class="failover">
        <span
          class:uk-alert-danger={value.input.backupStatus === 'OFFLINE'}
          class:uk-alert-warning={value.input.backupStatus === 'INITIALIZING'}
          class:uk-alert-success={value.input.backupStatus === 'ONLINE'}
        >
          <i class="fas fa-arrow-right" title="Accepts backup RTMP stream" />
        </span>
        <span>rtmp://{public_host}/{value.input.name}/backup</span>
        <span class="resulting">
          {#if value.input.status === 'ONLINE'}
            <span
              ><i
                class="fas fa-circle uk-alert-success"
                title="Failover RTMP stream"
              /></span
            >
          {:else if value.input.status === 'INITIALIZING'}
            <span
              ><i
                class="fas fa-dot-circle uk-alert-warning"
                title="Failover RTMP stream"
              /></span
            >
          {:else}
            <span
              ><i
                class="far fa-dot-circle uk-alert-danger"
                title="Failover RTMP stream"
              /></span
            >
          {/if}
          <span>rtmp://{public_host}/{value.input.name}/in</span>
        </span>
      </div>
    {/if}

    {#if value.outputs && value.outputs.length > 0}
      <div class="uk-grid uk-grid-small" uk-grid>
        {#each value.outputs as output}
          <Output
            input_id={value.id}
            value={output}
            hidden={!showAll && !showFiltered[output.status]}
          />
        {/each}
      </div>
    {/if}
  </div>
</template>

<style lang="stylus">
  .uk-section
    position: relative
    margin-top: 20px
    padding-left: 10px
    padding-right: @padding-left

    .uk-close
      float: right
      margin-top: 5px

    .uk-button-small
      float: right
      font-size: 0.7rem
      margin-top: -2px
      margin-right: 30px

    .total
      float: right
      margin-right: 30px
      .count
        text-align: right
        margin-right: 2px
        background-color: inherit
        padding: 1px 4px
        border-radius: 2px
        outline: none
        &:hover, &.enabled
          background-color: #cecece
        &:hover
          color: inherit
          text-decoration: none

    .export-import
      position: absolute
      right: -25px
      opacity: 0
      transition: opacity .3s ease
      color: #666
      outline: none
      &:hover
        text-decoration: none
        color: #444
        opacity: 1
    &:hover
      .export-import
        opacity: 1

    .fa-arrow-down, .fa-arrow-right
      font-size: 14px
      cursor: help

    .failover
      padding-left: 45px

      .resulting
        margin-left: 15px

        .fa-circle, .fa-dot-circle
          font-size: 14px
          cursor: help

    .uk-grid
      margin-top: 10px

    .label
      position: absolute
      top: -12px
      left: 0
      padding: 2px 10px
      border-top-left-radius: 4px
      border-top-right-radius: 4px
      background-color: #f8f8f8

  .edit-input
    margin-left: 6px
    color: #666
    opacity: 0
    transition: opacity .3s ease
    &:hover
      color: #444
  .uk-section:hover
    .edit-input
      opacity: 1
</style>
