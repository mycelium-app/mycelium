const WRAPPER = /\(\.\.\.a\) => \((\w+) = loadedWasm\._\["\1"\]\)\(\.\.\.a\)/g;

module.exports = function (source) {
    let patched = 0;
    const result = source.replace(WRAPPER, (_, name) => {
        patched++;
        return `(...a) => { try { return (${name} = loadedWasm._["${name}"])(...a) } catch (e) { throw e } }`;
    });
    if (patched === 0) throw new Error("skiko.mjs wrapper format changed, so the WebKit 329421 tail call workaround was not applied");
    return result;
};
