import { writable, get, Writable } from 'svelte/store';

import { sanitizeLabel, sanitizeUrl } from './util';

// Copied from 'svelte/store' as cannot be imported.
// See: https://github.com/sveltejs/svelte/pull/5887
/** Callback to inform of a value updates. */
declare type Subscriber<T> = (value: T) => void;
/** Unsubscribes from value updates. */
declare type Unsubscriber = () => void;
/** Callback to update a value. */
declare type Updater<T> = (value: T) => T;
/** Cleanup logic callback. */
declare type Invalidator<T> = (value?: T) => void;

/**
 * State of the modal window for adding and editing restreaming `Input`s.
 */
export class InputModalState {
  /**
   * ID of `Input` being edited in the [[`InputModal`]] at the moment.
   *
   * If `null` then new `Input` is being added.
   */
  edit_id: string | null = null;

  /**
   * Previous value of `Input` (`push_key` or `pull_url`) before it has been
   * edited in the [[`InputModal`]].
   */
  prev_value: string | null = null;

  /**
   * Previous label of `Input` (`push_key` or `pull_url`) before it has been
   * edited in the [[`InputModal`]].
   *
   * Empty string means no label.
   */
  prev_label: string = '';

  /**
   * Label to be assigned to the added/edited `Input`.
   *
   * Empty string means no label.
   */
  label: string = '';

  /**
   * Indicator whether "Pull" tab is chosen in the [[`InputModal`]].
   */
  is_pull: boolean = false;

  /**
   * Key of a local [SRS] endpoint to receive a live RTMP stream on in case of
   * `PushInput`.
   *
   * [SRS]: https://github.com/ossrs/srs
   */
  push_key: string = '';

  /**
   * Indicator whether a local backup [SRS] endpoint is required to receive a
   * live RTMP stream on in case of `FailoverPushInput`.
   *
   * [SRS]: https://github.com/ossrs/srs
   */
  failover: boolean = false;

  /**
   * Previous value of the `failover` indicator.
   */
  prev_failover: boolean | null = null;

  /**
   * URL to pull a live RTMP stream from in case of `PullInput`.
   *
   * [SRS]: https://github.com/ossrs/srs
   */
  pull_url: string = '';

  /**
   * Indicator whether the [[`InputModal`]] is visible (opened) at the moment.
   */
  visible: boolean = false;
}

/**
 * Shared reactive state of the modal window for adding and editing restreaming
 * `Input`s.
 */
export class InputModal implements Writable<InputModalState> {
  private state: Writable<InputModalState> = writable(new InputModalState());

  /** @inheritdoc */
  subscribe(
    run: Subscriber<InputModalState>,
    invalidate?: Invalidator<InputModalState>
  ): Unsubscriber {
    return this.state.subscribe(run, invalidate);
  }

  /** @inheritdoc */
  set(v: InputModalState) {
    if (v.pull_url !== '') {
      v.pull_url = sanitizeUrl(v.pull_url);
    }
    if (v.push_key !== '') {
      v.push_key = sanitizeUrl(v.push_key);
    }
    this.state.set(v);
  }

  /** @inheritdoc */
  update(updater: Updater<InputModalState>) {
    this.state.update(updater);
  }

  /**
   * Retrieves and returns current [[`InputModalState`]].
   *
   * @returns    Current value of [[`InputModalState`]].
   */
  get(): InputModalState {
    return get(this.state);
  }

  /**
   * Opens this [[`InputModal`]] window for adding a new `Input`.
   */
  openAdd() {
    this.update((v) => {
      v.visible = true;
      return v;
    });
  }

  /**
   * Opens this [[`InputModal`]] window for editing an existing `Input`.
   *
   * @param id          ID of the `Input` being edited.
   * @param val         Current value of `Input` (`push_key` or `pull_url`)
   *                    before editing.
   * @param label       Current label of `Input` before editing.
   * @param is_pull     Indicator whether the `Input` being edited is a
   *                    `PullInput`.
   * @param failover    Indicator whether the `PushInput` should have a
   *                    backup endpoint.
   */
  openEdit(
    id: string,
    val: string,
    label: string,
    is_pull: boolean,
    failover: boolean
  ) {
    this.update((v) => {
      v.edit_id = id;
      v.prev_value = val;
      v.is_pull = is_pull;
      if (v.is_pull) {
        v.pull_url = sanitizeUrl(val);
      } else {
        v.push_key = sanitizeUrl(val);
      }
      v.prev_failover = failover;
      v.failover = failover;
      if (!!label) {
        v.prev_label = label;
        v.label = sanitizeLabel(label);
      }
      v.visible = true;
      return v;
    });
  }

  /**
   * Switches the current active tab of this [[`InputModal`]] to "Pull".
   */
  switchPull() {
    this.update((v) => {
      v.is_pull = true;
      return v;
    });
  }

  /**
   * Switches the current active tab of this [[`InputModal`]] to "Push".
   */
  switchPush() {
    this.update((v) => {
      v.is_pull = false;
      return v;
    });
  }

  /**
   * Sanitizes the current label value being input in this [[`InputModal`]].
   */
  sanitizeLabel() {
    this.update((v) => {
      if (v.label !== '') {
        v.label = sanitizeLabel(v.label);
      }
      return v;
    });
  }

  /**
   * Closes this [[`InputModal`]] window.
   */
  close() {
    this.update((v) => {
      v.edit_id = null;
      v.prev_value = null;
      v.prev_label = '';
      v.prev_failover = null;
      v.label = '';
      v.push_key = '';
      v.pull_url = '';
      v.visible = false;
      return v;
    });
  }
}

/**
 * State of the modal window for adding restreaming `Output`s.
 */
export class OutputModalState {
  /**
   * ID of the `Input` to add new `Output` for.
   */
  input_id: string | null = null;

  /**
   * Indicator whether the "Multiple" tab is active in the [[`OutputModal`]].
   */
  multi: boolean = false;

  /**
   * Indicator whether the mixing form is active in the [[`OutputModal`]].
   */
  mixing: boolean = false;

  /**
   * Label to be assigned to the added `Output`.
   *
   * Empty string means no label.
   */
  label: string = '';

  /**
   * RTMP URL to restream a live RTMP stream to with the added `Output`.
   */
  url: string = '';

  /**
   * URL of a TeamSpeak channel to mix audio from with a live RTMP stream before
   * outputting it.
   */
  mix_url: string = '';

  /**
   * List of multiple labels and RTMP URLs to be added in a comma-separated
   * format.
   */
  list: string = '';

  /**
   * Indicator whether the [[`OutputModal`]] is visible (opened) at the
   * moment.
   */
  visible: boolean = false;
}

/**
 * Shared reactive state of the modal window for adding restreaming `Output`s.
 */
export class OutputModal implements Writable<OutputModalState> {
  private state: Writable<OutputModalState> = writable(new OutputModalState());

  /** @inheritdoc */
  subscribe(
    run: Subscriber<OutputModalState>,
    invalidate?: Invalidator<OutputModalState>
  ): Unsubscriber {
    return this.state.subscribe(run, invalidate);
  }

  /** @inheritdoc */
  set(v: OutputModalState) {
    if (v.url !== '') {
      v.url = sanitizeUrl(v.url);
    }
    if (v.mix_url !== '') {
      v.mix_url = sanitizeUrl(v.mix_url);
    }
    this.state.set(v);
  }

  /** @inheritdoc */
  update(updater: Updater<OutputModalState>) {
    this.state.update(updater);
  }

  /**
   * Retrieves and returns current [[`OutputModalState`]].
   *
   * @returns    Current value of [[`OutputModalState`]].
   */
  get(): OutputModalState {
    return get(this.state);
  }

  /**
   * Opens this [[`OutputModal`]] window for adding a new `Ouput`.
   *
   * @param id    ID of the `Input` that new `Ouput` being added to.
   */
  open(id: string) {
    this.update((v) => {
      v.input_id = id;
      v.visible = true;
      return v;
    });
  }

  /**
   * Switches the current active tab of this [[`OutputModal`]] to "Single".
   */
  switchSingle() {
    this.update((v) => {
      v.multi = false;
      return v;
    });
  }

  /**
   * Switches the current active tab of this [[`OutputModal`]] to "Multiple".
   */
  switchMulti() {
    this.update((v) => {
      v.multi = true;
      return v;
    });
  }

  /**
   * Toggles the mixing form of this [[`OutputModal`]].
   */
  toggleMixing() {
    this.update((v) => {
      v.mixing = !v.mixing;
      return v;
    });
  }

  /**
   * Sanitizes the current label value being input in this [[`OutputModal`]].
   */
  sanitizeLabel() {
    this.update((v) => {
      if (v.label !== '') {
        v.label = sanitizeLabel(v.label);
      }
      return v;
    });
  }

  /**
   * Closes this [[`OutputModal`]] window.
   */
  close() {
    this.update((v) => {
      v.input_id = null;
      v.label = '';
      v.url = '';
      v.mix_url = '';
      v.list = '';
      v.visible = false;
      return v;
    });
  }
}

/**
 * State of the modal window for adding exporting/importing `Inputs`s.
 */
export class ExportModalState {
  /**
   * ID of the `Input` to operate on.
   */
  input_id: string | null = null;

  /**
   * Current JSON value of the operated `Input`'s spec.
   */
  spec: string = '';

  /**
   * Previous JSON value of the operated `Input`'s spec.
   */
  prev_spec: string = '';

  /**
   * Indicator whether the [[`ExportModalModal`]] is visible (opened) at the
   * moment.
   */
  visible: boolean = false;
}

/**
 * Shared reactive state of the modal window for exporting/importing `Inputs`s.
 */
export class ExportModal implements Writable<ExportModalState> {
  private state: Writable<ExportModalState> = writable(new ExportModalState());

  /** @inheritdoc */
  subscribe(
    run: Subscriber<ExportModalState>,
    invalidate?: Invalidator<ExportModalState>
  ): Unsubscriber {
    return this.state.subscribe(run, invalidate);
  }

  /** @inheritdoc */
  set(v: ExportModalState) {
    this.state.set(v);
  }

  /** @inheritdoc */
  update(updater: Updater<ExportModalState>) {
    this.state.update(updater);
  }

  /**
   * Retrieves and returns current [[`ExportModalState`]].
   *
   * @returns    Current value of [[`ExportModalState`]].
   */
  get(): ExportModalState {
    return get(this.state);
  }

  /**
   * Opens this [[`ExportModal`]] window for exporting/importing an `Input`.
   *
   * @param id      ID of the `Input` to be exported/imported.
   * @param spec    Current `Input`'s spec received via GraphQL API.
   */
  async open(id: string | null, spec: string) {
    this.update((v) => {
      v.input_id = id;
      v.spec = spec;
      v.prev_spec = spec;
      v.visible = true;
      return v;
    });
  }

  /**
   * Closes this [[`ExportModal`]] window.
   */
  close() {
    this.update((v) => {
      v.input_id = null;
      v.spec = '';
      v.prev_spec = '';
      v.visible = false;
      return v;
    });
  }
}

/**
 * Global singleton instance of an [[`InputModal`]] window used by this
 * application.
 */
export const inputModal = new InputModal();

/**
 * Global singleton instance of an [[`OutputModal`]] window used by this
 * application.
 */
export const outputModal = new OutputModal();

/**
 * Global singleton instance of an [[`ExportModal`]] window used by this
 * application.
 */
export const exportModal = new ExportModal();
