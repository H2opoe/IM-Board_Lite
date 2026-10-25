import { stageReleaseResources } from "./windows-release-resources.mjs";
import { assertLiteResources } from "./lite-package-boundary.mjs";
import { releaseConfig } from "./release-config.mjs";
import { packagePortable } from "./windows-portable.mjs";

const config = releaseConfig();

try {
  packagePortable({
    appRoot: config.appRoot,
    productName: config.productName,
    releaseLabel: config.releaseLabel,
    templateVariant: "lite",
    freshTemplate: true,
    freshResources: true,
    executableName: "IM-Board-Lite.exe",
    binaryNames: ["im-board.exe", "IM-Board-Lite.exe"],
    removeLegacyExecutables: ["IM-Board.exe", "im-board.exe"],
    prepareStage(stage) { stageReleaseResources(config.appRoot,stage,["runtime","shared/third-party/process-wrap"]); },
    validateStage: assertLiteResources
  });
} catch (error) {
  console.error(`[windows-package] ${error.message}`);
  process.exitCode = 1;
}
