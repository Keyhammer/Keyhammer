import React from 'react';
import Link from '@docusaurus/Link';
import Layout from '@theme/Layout';
import Heading from '@theme/Heading';
import styles from './index.module.css';

const features = [
  {
    title: 'Fixed-point weighted edit costs',
    text: 'Edit costs are integers (16 is one ordinary edit), so search bounds are exact and results are identical on every platform. A neighbouring QWERTY key costs less than an arbitrary substitution. The cost values are provisional.',
  },
  {
    title: 'Exact best-first top-k over a trie',
    text: 'Terms live in a flat trie; each node carries one banded edit-distance row and a lower bound. The first terminal popped is always the best remaining one, and oracle tests compare the results with brute force.',
  },
  {
    title: 'no_std, no unsafe, zero dependencies',
    text: 'The core crate needs only alloc, forbids unsafe code and has no runtime dependencies.',
  },
];

export default function Home() {
  return (
    <Layout
      title="Keyhammer"
      description="Typo-tolerant top-k search over a compact trie, with keyboard-aware edit costs (unpublished prototype).">
      <header className={styles.hero}>
        <div className="container">
          <Heading as="h1" className="hero__title">
            Keyhammer
          </Heading>
          <p className="hero__subtitle">
            Typo-tolerant top-k search over a compact trie, with keyboard-aware edit costs.
          </p>
          <div className={styles.buttons}>
            <Link className="button button--primary button--lg" to="/docs/introduction">
              Read the docs
            </Link>
            <Link className="button button--secondary button--lg" to="/docs/results">
              M0 results
            </Link>
            <Link
              className="button button--secondary button--lg"
              href="https://github.com/Keyhammer/Keyhammer">
              GitHub
            </Link>
          </div>
        </div>
      </header>
      <main>
        <div className="container">
          <div className={`alert alert--warning ${styles.status}`} role="note">
            <strong>Status: unpublished prototype (M0).</strong> Nothing is published to
            crates.io or npm and the API is unstable. Speed against the previous engine and
            ranking quality against a simple edit-distance-plus-frequency baseline are reported,
            with their caveats, on the <Link to="/docs/results">results</Link> page. The
            project claims no novelty. See also the{' '}
            <Link to="/docs/prior-art">prior art</Link>.
          </div>
        </div>
        <section className={styles.features}>
          <div className="container">
            <div className="row">
              {features.map((f) => (
                <div key={f.title} className="col col--4">
                  <Heading as="h3">{f.title}</Heading>
                  <p>{f.text}</p>
                </div>
              ))}
            </div>
          </div>
        </section>
      </main>
    </Layout>
  );
}
