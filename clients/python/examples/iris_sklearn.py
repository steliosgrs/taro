"""Validate the curve-compare thesis on CPU with the Iris dataset (no GPU).

Trains two sklearn classifiers, logs per-class one-vs-rest PR curves to Taro via
the SDK core (the same `log_curve` path the YOLO adapter uses), then overlays the
two runs with `/curves/compare`. Iris is multiclass, so each class gets a PR curve
interpolated onto a shared recall grid; we log the macro-mean as `pr_curve` and the
per-class lines as `pr_curve_per_class`.

    cd server && cargo run                                  # terminal 1
    uv pip install scikit-learn                             # one-time
    python clients/python/examples/iris_sklearn.py           # terminal 2
"""

import os

import numpy as np
from sklearn.datasets import load_iris
from sklearn.ensemble import RandomForestClassifier
from sklearn.linear_model import LogisticRegression
from sklearn.metrics import (
    accuracy_score,
    average_precision_score,
    precision_recall_curve,
)
from sklearn.model_selection import train_test_split
from sklearn.preprocessing import label_binarize

import taro

# Shared recall grid so every per-class curve is the same length (overlayable).
RECALL_GRID = np.linspace(0.0, 1.0, 101)


def pr_on_grid(y_true_bin, scores):
    """One-vs-rest precision interpolated onto RECALL_GRID."""
    precision, recall, _ = precision_recall_curve(y_true_bin, scores)
    order = np.argsort(recall)  # np.interp needs ascending x
    return np.interp(RECALL_GRID, recall[order], precision[order])


def log_model(name, clf, params, X_train, X_test, y_train, y_test, class_names):
    clf.fit(X_train, y_train)
    proba = clf.predict_proba(X_test)
    acc = accuracy_score(y_test, clf.predict(X_test))

    y_bin = label_binarize(y_test, classes=range(len(class_names)))
    per_class_precision = [pr_on_grid(y_bin[:, c], proba[:, c]) for c in range(len(class_names))]
    aps = [float(average_precision_score(y_bin[:, c], proba[:, c])) for c in range(len(class_names))]

    with taro.start_run("iris-classification", name=name, params=params) as run:
        if not run.ok:
            raise SystemExit("could not start run — is the server up on :8080?")

        run.log_metric("accuracy", float(acc), step=0)
        run.log_metric("mAP", float(np.mean(aps)), step=0)

        # Headline PR = macro-mean across classes; per-class lines as an overlay.
        mean_precision = np.mean(per_class_precision, axis=0)
        run.log_curve("pr_curve", x=RECALL_GRID.tolist(), y=mean_precision.tolist(), step=0,
                      curve_type="pr", x_label="recall", y_label="precision")
        run.log_curve(
            "pr_curve_per_class", x=RECALL_GRID.tolist(), step=0, curve_type="pr",
            x_label="recall", y_label="precision",
            series=[{"name": class_names[c], "y": per_class_precision[c].tolist()}
                    for c in range(len(class_names))],
        )
        run.log_curve("per_class_ap", x=list(range(len(class_names))), y=aps, step=0,
                      curve_type="per_class", x_label="class", y_label="AP",
                      labels=list(class_names))

        print(f"logged {name:18} acc={acc:.3f}  mAP={np.mean(aps):.3f}  run={run.run_id}")
        return run.run_id


def main():
    taro.init(os.environ.get("TARO_URL", "http://localhost:8080"))
    data = load_iris()
    class_names = list(data.target_names)
    X_train, X_test, y_train, y_test = train_test_split(
        data.data, data.target, test_size=0.4, random_state=0, stratify=data.target
    )

    models = [
        ("logreg", LogisticRegression(max_iter=500), {"model": "logreg", "max_iter": 500}),
        ("random-forest", RandomForestClassifier(n_estimators=50, random_state=0),
         {"model": "random_forest", "n_estimators": 50}),
    ]
    run_ids = [log_model(n, c, p, X_train, X_test, y_train, y_test, class_names)
               for n, c, p in models]

    overlay = taro.compare_curves(run_ids, key="pr_curve")
    print("\n/curves/compare  (macro-mean PR, P@R=0.5):")
    for r in overlay["runs"]:
        print(f"  {r['run_name']:18} P@R=0.5 = {r['data']['y'][50]:.3f}")
    print("\nThesis validated on CPU: two models' PR curves overlaid as data. ✅")


if __name__ == "__main__":
    main()
