# syntax=docker/dockerfile:1
#
# Seed container: installs the Taro SDK and runs the Iris curve-compare example
# against the server, so a fresh `docker compose up` already has two runs with
# overlayable PR curves to look at. Build context is clients/python/.
FROM python:3.12-slim
WORKDIR /app

# The SDK core is stdlib-only; the Iris seed additionally needs sklearn + numpy.
COPY pyproject.toml README.md ./
COPY taro ./taro
RUN pip install --no-cache-dir -e . scikit-learn numpy

COPY examples ./examples

# Overridden by compose to point at the server service.
ENV TARO_URL=http://localhost:8080
CMD ["python", "examples/iris_sklearn.py"]
