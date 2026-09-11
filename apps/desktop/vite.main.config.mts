import { defineConfig, loadEnv } from "vite";
import { resolve } from "path";
import { execSync } from "node:child_process";
import { posthogSourceMapPlugins } from "./vite.posthog";

function getSelectedInputMethod(): string | undefined {
  const argv = process.argv;
  for (let i = 0; i < argv.length; i++) {
    if (argv[i] === "--input-method" && i + 1 < argv.length) {
      return argv[i + 1];
    }
    if (argv[i].startsWith("--input-method=")) {
      return argv[i].split("=")[1];
    }
  }
  return process.env.AMICAL_INPUT_METHOD;
}

// https://vitejs.dev/config
export default defineConfig(({ mode }) => {
  const env = loadEnv(mode, process.cwd(), "");

  // When building on Linux with a non-default input method, rebuild linux-helper with the requested feature
  if (process.platform === "linux") {
    const inputMethod = getSelectedInputMethod();
    const cargoFeatures: string[] = [];
    if (inputMethod && inputMethod !== "clipboard") {
      cargoFeatures.push(inputMethod);
    }
    if (cargoFeatures.length > 0) {
      const featureArg = `--features ${cargoFeatures.join(",")}`;
      const helperDir = resolve(
        __dirname,
        "../../packages/native-helpers/linux-helper",
      );
      const isRelease = mode === "production";
      const releaseFlag = isRelease ? "--release" : "";
      const targetSubdir = isRelease ? "release" : "debug";
      console.log(`[vite.main] Rebuilding linux-helper with ${featureArg}...`);
      execSync(
        `cargo build ${releaseFlag} ${featureArg} && mkdir -p bin && cp target/${targetSubdir}/linux-helper bin/linux-helper`,
        { cwd: helperDir, stdio: "inherit" },
      );
    }
  }

  return {
    plugins: posthogSourceMapPlugins(),
    define: {
      __BUNDLED_POSTHOG_API_KEY: JSON.stringify(env.POSTHOG_API_KEY || ""),
      __BUNDLED_POSTHOG_HOST: JSON.stringify(env.POSTHOG_HOST || ""),
      __BUNDLED_TELEMETRY_ENABLED: JSON.stringify(
        env.TELEMETRY_ENABLED !== "false",
      ),
      __BUNDLED_AUTH_CLIENT_ID: JSON.stringify(env.AUTH_CLIENT_ID || ""),
      __BUNDLED_AUTH_AUTHORIZATION_ENDPOINT: JSON.stringify(
        env.AUTHORIZATION_ENDPOINT || "",
      ),
      __BUNDLED_AUTH_TOKEN_ENDPOINT: JSON.stringify(
        env.AUTH_TOKEN_ENDPOINT || "",
      ),
      __BUNDLED_API_ENDPOINT: JSON.stringify(env.API_ENDPOINT || ""),
      __BUNDLED_CORE_API_URL: JSON.stringify(env.CORE_API_URL || ""),
      __BUNDLED_FEEDBACK_SURVEY_ID: JSON.stringify(
        env.FEEDBACK_SURVEY_ID || "",
      ),
      __BUNDLED_AUTH_REDIRECT_URI: JSON.stringify(env.AUTH_REDIRECT_URI || ""),
    },
    build: {
      rollupOptions: {
        input: {
          main: resolve(__dirname, "src/main/main.ts"),
          "whisper-worker-fork": resolve(
            __dirname,
            "src/pipeline/providers/transcription/whisper-worker-fork.ts",
          ),
        },
        output: {
          entryFileNames: "[name].js",
        },
        external: [
          "@amical/whisper-wrapper",
          "better-sqlite3",
          "onnxruntime-node",
          /^node:/,
          /^electron$/,
        ],
      },
    },
    resolve: {
      alias: {
        "@": resolve(__dirname, "src"),
      },
    },
    optimizeDeps: {
      exclude: ["better-sqlite3", "@amical/whisper-wrapper", "drizzle-orm"],
    },
  };
});
