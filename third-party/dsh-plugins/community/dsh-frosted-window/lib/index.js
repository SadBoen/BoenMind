//#region src/index.ts
/**
* Host half of dsh-frosted-window.
*
* The Cordis loader must mount a Node entry so the client-modules scanner
* can discover `dsh.client` on this package. All presentation lives in the
* browser half — this apply is intentionally empty.
*/
const name = "dsh-frosted-window";
/** Mount the package so the Web client graph includes this plugin. */
function apply() {}
//#endregion
export { apply, name };
