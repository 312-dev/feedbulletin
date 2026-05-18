<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { EditorState, Compartment } from "@codemirror/state";
  import { EditorView, keymap, lineNumbers, highlightActiveLineGutter, highlightActiveLine } from "@codemirror/view";
  import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
  import { searchKeymap, highlightSelectionMatches } from "@codemirror/search";
  import { yaml as yamlLang } from "@codemirror/lang-yaml";
  import { lintGutter, linter, type Diagnostic } from "@codemirror/lint";
  import { bracketMatching, foldGutter, foldEffect, indentOnInput, indentUnit, syntaxHighlighting, defaultHighlightStyle } from "@codemirror/language";
  import { oneDark } from "@codemirror/theme-one-dark";
  import { validateFeedsYaml } from "$lib/feeds-schema";

  type Props = {
    value: string;
    onChange: (next: string) => void;
    onDiagnostics?: (diags: Diagnostic[]) => void;
    dark?: boolean;
  };
  let { value, onChange, onDiagnostics, dark = false }: Props = $props();

  let container: HTMLDivElement | null = $state(null);
  let view: EditorView | null = null;
  const themeC = new Compartment();
  let lastEmitted = "";

  function makeLinter() {
    return linter((v) => {
      const diags = validateFeedsYaml(v.state.doc.toString());
      onDiagnostics?.(diags);
      return diags;
    }, { delay: 200 });
  }

  function makeExtensions(darkMode: boolean) {
    return [
      lineNumbers(),
      highlightActiveLineGutter(),
      foldGutter(),
      history(),
      indentOnInput(),
      bracketMatching(),
      highlightActiveLine(),
      highlightSelectionMatches(),
      indentUnit.of("  "),
      yamlLang(),
      syntaxHighlighting(defaultHighlightStyle, { fallback: true }),
      lintGutter(),
      makeLinter(),
      keymap.of([
        indentWithTab,
        ...defaultKeymap,
        ...historyKeymap,
        ...searchKeymap,
      ]),
      EditorView.lineWrapping,
      EditorView.updateListener.of((u) => {
        if (u.docChanged) {
          const text = u.state.doc.toString();
          if (text !== lastEmitted) {
            lastEmitted = text;
            onChange(text);
          }
        }
      }),
      themeC.of(darkMode ? oneDark : []),
    ];
  }

  $effect(() => {
    if (!view) return;
    view.dispatch({
      effects: themeC.reconfigure(dark ? oneDark : []),
    });
  });

  $effect(() => {
    if (!view) return;
    const current = view.state.doc.toString();
    if (value !== current && value !== lastEmitted) {
      view.dispatch({
        changes: { from: 0, to: current.length, insert: value },
      });
      lastEmitted = value;
    }
  });

  /** Fold each top-level category entry in feeds.yaml on initial open so the
   * editor isn't overwhelming for a 100-forum config.
   *
   * WHY indent-based instead of `foldable()`: the YAML language extension's
   * syntax tree is built incrementally and isn't ready for lines past the
   * first chunk by the time onMount fires. `foldable()` would return null
   * for later categories, leaving them visually open. Walking by indent
   * works regardless of parse progress.
   *
   * The shape we rely on is fixed:
   *   `  - name: <category>`   (indent 2)
   *   `    forums:`            (indent 4)
   *   `      - kind: …`        (indent 6 — children)
   * so "block ends when we hit the next line at indent ≤ 2." */
  function foldAllCategoriesOnce(v: EditorView) {
    const doc = v.state.doc;
    const effects = [];
    for (let i = 1; i <= doc.lines; i++) {
      const line = doc.line(i);
      // `- name:` at any leading indent (serde_yaml writes list items at
      // indent 0; hand-written YAML often uses 2). Within `forums:` items
      // are `- title:` / `- kind:`, never `- name:`, so this is unambiguous.
      if (!/^\s*- name:\s/.test(line.text)) continue;
      const catIndent = line.text.length - line.text.trimStart().length;
      let endLine = doc.lines;
      for (let j = i + 1; j <= doc.lines; j++) {
        const next = doc.line(j);
        if (next.text.trim() === "") continue;
        const nextIndent = next.text.length - next.text.trimStart().length;
        if (nextIndent <= catIndent) { endLine = j - 1; break; }
      }
      while (endLine > i && doc.line(endLine).text.trim() === "") endLine--;
      if (endLine > i) {
        effects.push(foldEffect.of({ from: line.to, to: doc.line(endLine).to }));
      }
    }
    if (effects.length > 0) v.dispatch({ effects });
  }

  onMount(() => {
    if (!container) return;
    lastEmitted = value;
    view = new EditorView({
      parent: container,
      state: EditorState.create({
        doc: value,
        extensions: makeExtensions(dark),
      }),
    });
    // Defer until after the language extension has parsed the doc, otherwise
    // foldable() returns null because the syntax tree hasn't built yet.
    requestAnimationFrame(() => {
      if (view) foldAllCategoriesOnce(view);
    });
  });

  onDestroy(() => {
    view?.destroy();
    view = null;
  });
</script>

<div class="yaml-editor" bind:this={container}></div>

<style>
  .yaml-editor {
    width: 100%;
    border: 1px solid var(--vb-border);
    border-radius: 3px;
    overflow: hidden;
    background: var(--vb-content-bg);
  }
  .yaml-editor :global(.cm-editor) {
    height: 540px;
    font-family: "SF Mono", Menlo, Consolas, monospace;
    font-size: 12px;
    line-height: 1.55;
  }
  .yaml-editor :global(.cm-editor.cm-focused) {
    outline: none;
  }
  .yaml-editor :global(.cm-scroller) {
    overflow: auto;
  }
  /* CodeMirror's lint marks are red squiggles by default — bump contrast on dark themes. */
  .yaml-editor :global(.cm-diagnostic-error) {
    border-left: 3px solid #d33;
  }
  .yaml-editor :global(.cm-diagnostic-warning) {
    border-left: 3px solid #c8a04a;
  }
</style>
