import type { OpenClawPluginApi } from "openclaw/plugin-sdk";
import { pikachatPlugin } from "./src/channel.js";
import { pikachatPluginConfigSchema } from "./src/config-schema.js";
import { setPikachatRuntime } from "./src/runtime.js";

const plugin = {
  id: "pikachat-openclaw",
  name: "Pikachat",
  description: "Pikachat MLS group messaging over Nostr (Rust sidecar)",
  configSchema: pikachatPluginConfigSchema,
  register(api: OpenClawPluginApi) {
    setPikachatRuntime(api.runtime);
    api.registerChannel({ plugin: pikachatPlugin });
  },
};

export default plugin;
