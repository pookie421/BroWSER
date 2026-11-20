PYTHON ?= python3
CARGO ?= cargo

.PHONY: check validate test engine-check

check: test validate

validate:
	$(PYTHON) tools/check_plan.py

test:
	$(PYTHON) -m unittest discover -s tests -v

engine-check:
	$(CARGO) fmt --all -- --check
	$(CARGO) test --locked --offline --all-targets
	$(CARGO) clippy --locked --offline --all-targets -- -D warnings
	$(CARGO) build --locked --offline
	$(PYTHON) tools/run_m1_bootstrap.py --binary target/debug/browser-engine --repeat 100
