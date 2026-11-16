PYTHON ?= python3

.PHONY: check validate test

check: test validate

validate:
	$(PYTHON) tools/check_plan.py

test:
	$(PYTHON) -m unittest discover -s tests -v
