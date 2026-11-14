// Safari 27.2 passes wrong float arguments on a JavaScript tail call into Wasm (WebKit bug 329421,
// Compose CMP-10986), so nothing paints. Skiko's export wrappers are such tail calls until the fix ships.
const path = require("path");
const TAIL_CALL_WRAPPER = /\(\.\.\.([\w$]+)\)\s*=>\s*\(\s*[\w$]+\s*=\s*[\w$]+\._(?:\.[\w$]+|\[[^\]]+\])\s*\)\s*\(\.\.\.\1\)/;

config.module.rules.push({
    test: /[\\/]skiko\.mjs$/,
    loader: path.resolve(__dirname, "../../../../web/webpack/skiko-no-tail-call-loader.cjs"),
});

config.plugins.push({
    apply(compiler) {
        compiler.hooks.emit.tap("SkikoNoTailCallCheck", (compilation) => {
            for (const [name, asset] of Object.entries(compilation.assets)) {
                if (name.endsWith(".js") && TAIL_CALL_WRAPPER.test(asset.source().toString())) {
                    compilation.errors.push(new Error(`${name} still calls Skiko through a tail call, which Safari 27.2 runs wrongly (WebKit bug 329421)`));
                }
            }
        });
    },
});
