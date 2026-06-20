import clsx from 'clsx';
import Heading from '@theme/Heading';
import styles from './styles.module.css';

type FeatureItem = {
  title: string;
  Svg: React.ComponentType<React.ComponentProps<'svg'>>;
  description: JSX.Element;
};

// Placeholder copy + demo SVGs — swap the art and wording when you shape the site.
const FeatureList: FeatureItem[] = [
  {
    title: 'Curve-native',
    Svg: require('@site/static/img/undraw_docusaurus_mountain.svg').default,
    description: (
      <>
        A metric value can be a whole curve — PR curves stored as structured
        data, so N runs overlay and compare. Not an opaque PNG.
      </>
    ),
  },
  {
    title: 'Self-hosted &amp; lean',
    Svg: require('@site/static/img/undraw_docusaurus_tree.svg').default,
    description: (
      <>
        A single Rust binary (axum + SQLite or Postgres). Tiny footprint,
        one-command Docker stack, artifacts streamed straight to blob storage.
      </>
    ),
  },
  {
    title: 'Reproducible by design',
    Svg: require('@site/static/img/undraw_docusaurus_react.svg').default,
    description: (
      <>
        A versioned, content-addressed registry links every run to the exact
        config and dataset recipe it ran from — provenance both ways.
      </>
    ),
  },
];

function Feature({title, Svg, description}: FeatureItem) {
  return (
    <div className={clsx('col col--4')}>
      <div className="text--center">
        <Svg className={styles.featureSvg} role="img" />
      </div>
      <div className="text--center padding-horiz--md">
        <Heading as="h3">{title}</Heading>
        <p>{description}</p>
      </div>
    </div>
  );
}

export default function HomepageFeatures(): JSX.Element {
  return (
    <section className={styles.features}>
      <div className="container">
        <div className="row">
          {FeatureList.map((props, idx) => (
            <Feature key={idx} {...props} />
          ))}
        </div>
      </div>
    </section>
  );
}
