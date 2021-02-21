export class Restream {
  label: string | null;
  enabled: boolean;
  input: PushInput | PullInput;
  outputs: Output[];

  constructor(val: any) {
    this.label = val.label;
    this.enabled = val.enabled;
    switch (val.input.__typename) {
      case 'PushInput':
        this.input = new PushInput(val.input);
        break;
      case 'FailoverPushInput':
        this.input = new FailoverPushInput(val.input);
        break;
      case 'PullInput':
        this.input = new PullInput(val.input);
        break;
    }
    this.outputs = val.outputs.map((v) => new Output(v));
  }
}

export class PushInput {
  kind: string = "PushInput";
  name: string;

  constructor(val: any) {
    this.name = val.name;
  }
}

export class FailoverPushInput {
  kind: string = "FailoverPushInput";
  name: string;

  constructor(val: any) {
    this.name = val.name;
  }
}

export class PullInput {
  kind: string = "PullInput";
  src: URL;

  constructor(val: any) {
    this.src = val.src;
  }
}

export class Output {
  dst: URL;
  label: string | null;
  enabled: boolean;
  volume: number;
  mixins: Mixin[];

  constructor(val: any) {
    console.log(val);
    this.dst = val.dst;
    this.label = val.label;
    this.enabled = val.enabled;
    this.volume = val.volume;
    this.mixins = val.mixins.map((v) => new Mixin(v));
  }
}

export class Mixin {
  src: URL;
  volume: number;
  delay: number;

  constructor(val: any) {
    this.src = val.src;
    this.volume = val.volume;
    this.delay = val.delay;
  }
}
