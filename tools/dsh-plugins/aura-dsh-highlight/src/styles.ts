/**
 * Viewport CSS for the Aura code viewer.
 *
 * Two concerns, one stylesheet:
 *
 *   1. `--aura-*` color variables, in the light values on `:root` and the dark
 *      values under `body[data-ds-dark-theme]`. Tokens reference these
 *      variables (see theme.ts), so a theme switch repaints without re-running
 *      the tokenizer.
 *
 *   2. The viewer's own layout, scoped under `[data-aura-code]` so it cannot
 *      collide with host styles, and consuming DSH's `--dsw-*` tokens for
 *      surfaces and text so it reads as part of the host.
 *
 * Installed as `<style data-plugin-css="...">` tags the way DSH's own
 * dsh-client-ui-theme does it, which is what makes the HMR receiver track
 * changes.
 */

import { themeVariableCss } from './theme.js';

/** Component CSS, scoped under `[data-aura-code]`. */
export const VIEWER_CSS = /* css */ `
[data-aura-code]{
  display:flex;
  flex-direction:column;
  min-height:0;
  height:100%;
  font-family:ui-monospace,SFMono-Regular,"SF Mono",Menlo,Consolas,"Liberation Mono",monospace;
  font-size:13px;
  line-height:20px;
  color:var(--aura-foreground);
}
[data-aura-code] [data-aura-banner]{
  flex:none;
  display:flex;
  align-items:center;
  gap:8px;
  padding:5px 12px;
  border-bottom:1px solid var(--dsw-alias-border-l2,#21262d);
  background:var(--dsw-alias-markdown-code-block-banner,transparent);
  font-family:var(--dsw-font-family,sans-serif);
  font-size:12px;
  line-height:18px;
  color:var(--dsw-alias-label-secondary,inherit);
}
[data-aura-code] [data-aura-lang]{
  font-family:ui-monospace,SFMono-Regular,Menlo,Consolas,monospace;
  letter-spacing:0.02em;
}
[data-aura-code] [data-aura-status]{
  margin-left:auto;
  color:var(--dsw-alias-label-tertiary,inherit);
}
[data-aura-code] [data-aura-copy]{
  margin-left:auto;
  display:inline-flex;
  align-items:center;
  gap:5px;
  padding:2px 8px;
  border:1px solid var(--dsw-alias-border-l2,#21262d);
  border-radius:6px;
  background:transparent;
  color:var(--dsw-alias-label-secondary,inherit);
  font-family:var(--dsw-font-family,sans-serif);
  font-size:12px;
  line-height:16px;
  cursor:pointer;
}
[data-aura-code] [data-aura-copy]:hover{
  background:var(--dsw-alias-interactive-bg-hover,rgba(127,127,127,0.12));
  color:var(--dsw-alias-label-primary,inherit);
}
[data-aura-code] [data-aura-copy][data-copied="true"]{
  color:var(--dsw-alias-state-success-primary,inherit);
  border-color:var(--dsw-alias-state-success-primary,inherit);
}
[data-aura-code] [data-aura-scroll]{
  flex:1 1 auto;
  min-height:0;
  overflow:auto;
  padding:8px 0 12px;
  scrollbar-color:var(--dsw-alias-scrollbar-bg-l1,transparent) transparent;
}
[data-aura-code] [data-aura-grid]{
  display:grid;
  grid-template-columns:max-content minmax(0,1fr);
  grid-auto-flow:row;
  grid-auto-rows:min-content;
  align-items:baseline;
  width:100%;
}
[data-aura-code] [data-aura-num]{
  padding:0 12px 0 16px;
  text-align:right;
  white-space:pre;
  color:var(--dsw-alias-label-tertiary,inherit);
  user-select:none;
}
[data-aura-code] [data-aura-text]{
  padding-right:16px;
  white-space:pre;
  overflow-wrap:normal;
  word-break:normal;
}
[data-aura-code][data-wrap="true"] [data-aura-text]{
  white-space:pre-wrap;
  overflow-wrap:anywhere;
  word-break:break-word;
}
[data-aura-code] [data-aura-empty]{
  padding:16px;
  color:var(--dsw-alias-label-tertiary,inherit);
  font-family:var(--dsw-font-family,sans-serif);
  font-size:13px;
}
`;

/**
 * Every stylesheet this plugin installs.
 *
 * The order matters: variables first, then the component that consumes them.
 */
export const PLUGIN_CSS: ReadonlyArray<readonly [string, string]> = [
  ['aura-theme-variables.css', themeVariableCss()],
  ['aura-code-viewer.css', VIEWER_CSS],
];
