class Recorder extends AudioWorkletProcessor {
  constructor() {
    super();
    this.buffer = new Float32Array(2048);
    this.offset = 0;
    this.stopped = false;
    this.port.onmessage = ({ data }) => {
      if (data === "stop") {
        this.stopped = true;
        this.flush();
        this.port.postMessage("stopped");
      }
    };
  }
  flush() {
    if (this.offset) this.port.postMessage(this.buffer.slice(0, this.offset));
    this.offset = 0;
  }
  process(inputs) {
    if (this.stopped) return false;
    const channels = inputs[0];
    if (!channels?.length) return true;
    for (let i = 0; i < channels[0].length; i++) {
      let sample = 0;
      for (const channel of channels) sample += channel[i] / channels.length;
      this.buffer[this.offset++] = sample;
      if (this.offset === this.buffer.length) this.flush();
    }
    // Outputs remain silent; the microphone is never played through the speakers.
    return true;
  }
}
registerProcessor("recorder", Recorder);
