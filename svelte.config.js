import adapter from "@sveltejs/adapter-static";
import preprocess from "svelte-preprocess";

/** @type {import('@sveltejs/kit').Config} */
const config = {
  preprocess: preprocess(),

  kit: {
    adapter: adapter({ fallback: "index.html" }),
    prerender: { enabled: false },
    ssr: false,
    target: "#svelte",
  },
};

export default config;
