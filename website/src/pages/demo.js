import React, { useCallback, useEffect, useMemo, useState } from 'react';
import BrowserOnly from '@docusaurus/BrowserOnly';
import Link from '@docusaurus/Link';
import Layout from '@theme/Layout';
import Heading from '@theme/Heading';
import useBaseUrl from '@docusaurus/useBaseUrl';
import styles from './demo.module.css';

const ERROR = 0xffffffff;
const COST_UNIT = 16;
const BUILD_COMMAND =
  'cargo build -p keyhammer-wasm --target wasm32-unknown-unknown --profile wasm\n' +
  'mkdir -p website/static/wasm\n' +
  'cp target/wasm32-unknown-unknown/wasm/keyhammer_wasm.wasm website/static/wasm/keyhammer.wasm';

// Thin wrapper over the C-ABI of bindings/wasm (see its crate documentation).
function makeEngine(exports) {
  const enc = new TextEncoder();
  const dec = new TextDecoder();
  function withBuffer(text, f) {
    const data = enc.encode(text);
    const ptr = exports.kh_alloc(data.length);
    if (data.length > 0 && ptr === 0) throw new Error('out of memory');
    new Uint8Array(exports.memory.buffer, ptr, data.length).set(data);
    try {
      return f(ptr, data.length);
    } finally {
      exports.kh_free(ptr, data.length);
    }
  }
  return {
    build(text) {
      return withBuffer(text, (p, n) => exports.kh_build(p, n)) >>> 0;
    },
    search(query, { k, budget, ranking }) {
      const t0 = performance.now();
      const n = withBuffer(query, (p, len) => exports.kh_search(p, len, k, budget, ranking)) >>> 0;
      const ms = performance.now() - t0;
      if (n === ERROR) return { error: true, ms };
      const text = dec.decode(
        new Uint8Array(exports.memory.buffer, exports.kh_results_ptr(), exports.kh_results_len()),
      );
      const [header, ...lines] = text.split('\n').filter((l) => l.length > 0);
      const [nodes, truncated] = (header ?? '0\t0').split('\t').map(Number);
      const hits = lines.map((l) => {
        const [term, cost, weight] = l.split('\t');
        return { term, cost: Number(cost), weight: Number(weight) };
      });
      return { hits, nodes, truncated: truncated === 1, ms };
    },
  };
}

function Missing({ reason }) {
  return (
    <div className="alert alert--info" role="status">
      <p>
        <strong>The WebAssembly module is not available{reason ? ` (${reason})` : ''}.</strong>{' '}
        It is built by CI when the site is published. For a local build of the site, build the
        crate and copy the module into <code>website/static/wasm/</code> first, from the
        repository root:
      </p>
      <pre>
        <code>{BUILD_COMMAND}</code>
      </pre>
    </div>
  );
}

function Demo() {
  const wasmUrl = useBaseUrl('/wasm/keyhammer.wasm');
  const sampleUrl = useBaseUrl('/demo-data/sample-terms.tsv');
  const [engine, setEngine] = useState(null);
  const [loadError, setLoadError] = useState(null);
  const [source, setSource] = useState('sample');
  const [sampleText, setSampleText] = useState(null);
  const [customText, setCustomText] = useState('');
  const [indexed, setIndexed] = useState({ terms: 0, ms: 0, version: 0 });
  const [query, setQuery] = useState('javasript');
  const [budget, setBudget] = useState(32);
  const [ranking, setRanking] = useState(0);
  const [k, setK] = useState(10);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const res = await fetch(wasmUrl);
        if (!res.ok) throw new Error(`HTTP ${res.status}`);
        const { instance } = await WebAssembly.instantiate(await res.arrayBuffer(), {});
        if (!cancelled) setEngine(makeEngine(instance.exports));
      } catch (e) {
        if (!cancelled) setLoadError(String(e.message ?? e));
      }
    })();
    (async () => {
      try {
        const res = await fetch(sampleUrl);
        if (!res.ok) throw new Error(`HTTP ${res.status}`);
        const text = await res.text();
        if (!cancelled) setSampleText(text);
      } catch {
        if (!cancelled) setSampleText('');
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [wasmUrl, sampleUrl]);

  const buildIndex = useCallback(
    (text) => {
      if (!engine || text === null) return;
      const t0 = performance.now();
      const terms = engine.build(text);
      const ms = performance.now() - t0;
      setIndexed((prev) => ({ terms, ms, version: prev.version + 1 }));
    },
    [engine],
  );

  // The sample dictionary is (re)built when it is selected; a pasted one only
  // when the button is pressed, so that typing in the textarea stays cheap.
  useEffect(() => {
    if (source === 'sample') buildIndex(sampleText);
  }, [source, sampleText, buildIndex]);

  const result = useMemo(() => {
    if (!engine || indexed.terms === 0) return null;
    return engine.search(query, { k, budget, ranking });
    // indexed.version re-runs the search after every build.
  }, [engine, indexed, query, k, budget, ranking]);

  if (loadError) return <Missing reason={loadError} />;
  if (!engine) return <p>Loading the WebAssembly module...</p>;

  return (
    <div>
      <div className={styles.controls}>
        <label className={styles.query}>
          Query (try a typo)
          <input
            type="text"
            value={query}
            autoFocus
            spellCheck={false}
            autoCapitalize="off"
            autoComplete="off"
            maxLength={128}
            onChange={(e) => setQuery(e.target.value)}
          />
        </label>
        <label>
          Budget
          <select value={budget} onChange={(e) => setBudget(Number(e.target.value))}>
            <option value={32}>32 (default, about two edits)</option>
            <option value={48}>48 (more recall)</option>
          </select>
        </label>
        <label>
          Ranking
          <select value={ranking} onChange={(e) => setRanking(Number(e.target.value))}>
            <option value={0}>Coarse (default)</option>
            <option value={1}>Exact</option>
          </select>
        </label>
        <label>
          k
          <input
            type="number"
            min={1}
            max={50}
            value={k}
            onChange={(e) => setK(Math.min(50, Math.max(1, Number(e.target.value) || 1)))}
          />
        </label>
      </div>

      <fieldset className={styles.dictionary}>
        <legend>Dictionary</legend>
        <label className={styles.radio}>
          <input
            type="radio"
            name="source"
            checked={source === 'sample'}
            onChange={() => setSource('sample')}
          />
          Built-in sample (about 1,200 words, programming terms and countries)
        </label>
        <label className={styles.radio}>
          <input
            type="radio"
            name="source"
            checked={source === 'custom'}
            onChange={() => {
              setSource('custom');
              buildIndex(customText);
            }}
          />
          Paste your own
        </label>
        {source === 'custom' && (
          <div>
            <textarea
              className={styles.textarea}
              rows={8}
              spellCheck={false}
              placeholder={'one term per line, optional TAB weight (0 to 65535)\njavascript\t10\ntypescript\t8'}
              value={customText}
              onChange={(e) => setCustomText(e.target.value)}
            />
            <button
              type="button"
              className="button button--primary button--sm"
              onClick={() => buildIndex(customText)}>
              Use this dictionary
            </button>
          </div>
        )}
        <p className={styles.note}>
          Everything runs in your browser: the dictionary and the queries are not uploaded
          anywhere. {indexed.terms > 0
            ? `Index: ${indexed.terms} distinct terms, built in ${indexed.ms.toFixed(1)} ms.`
            : source === 'custom'
              ? 'No index yet: paste terms and press the button.'
              : 'Loading the sample dictionary...'}
        </p>
      </fieldset>

      {result && <Results result={result} />}
    </div>
  );
}

function Results({ result }) {
  if (result.error) {
    return (
      <p className="alert alert--danger" role="status">
        The search was rejected (the query may be longer than 128 bytes).
      </p>
    );
  }
  return (
    <div>
      <p className={styles.note}>
        {result.hits.length} hit{result.hits.length === 1 ? '' : 's'} in{' '}
        {result.ms.toFixed(3)} ms (one call, measured with <code>performance.now()</code>, whose
        resolution the browser may coarsen), {result.nodes} trie nodes expanded
        {result.truncated ? ', stopped early by the node limit' : ''}.
      </p>
      {result.hits.length === 0 ? (
        <p>No term within the budget.</p>
      ) : (
        <table className={styles.table}>
          <thead>
            <tr>
              <th>#</th>
              <th>Term</th>
              <th title="The exact cost rounded up to whole units of 16">Cost (units)</th>
              <th title="Fixed point: 16 is one ordinary edit">Exact cost (16 = one edit)</th>
              <th>Weight</th>
            </tr>
          </thead>
          <tbody>
            {result.hits.map((h, i) => (
              <tr key={h.term}>
                <td>{i + 1}</td>
                <td>
                  <code>{h.term}</code>
                </td>
                <td>{Math.ceil(h.cost / COST_UNIT)}</td>
                <td>{h.cost}</td>
                <td>{h.weight}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}

export default function DemoPage() {
  return (
    <Layout
      title="Demo"
      description="Try the Keyhammer typo-tolerant search in the browser (WebAssembly, unpublished prototype).">
      <main className={`container ${styles.page}`}>
        <Heading as="h1">Live demo</Heading>
        <div className="alert alert--warning" role="note">
          <strong>Unpublished prototype.</strong> This page runs the Keyhammer core compiled to
          WebAssembly, entirely in your browser, and loads nothing from third parties. The engine
          currently expects lowercase ASCII <code>a-z</code> (ASCII letters are lower-cased; other
          bytes are compared verbatim), and the results depend on provisional edit costs that
          have not been calibrated. See the <Link to="/docs/results">results</Link> page for what
          has been measured and its caveats.
        </div>
        <BrowserOnly fallback={<p>Loading the demo...</p>}>{() => <Demo />}</BrowserOnly>
      </main>
    </Layout>
  );
}
