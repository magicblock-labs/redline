CONFIG ?= ./config.toml

RUN_CMD = @cargo run --release --quiet --bin

build:
	@cargo build --release --bins

bench:
	$(RUN_CMD) redline $(CONFIG)

OUTPUT ?=

report:
	$(RUN_CMD) redline-assist report $(OUTPUT)

prepare:
	$(RUN_CMD) redline-assist prepare $(CONFIG)

THIS ?=
THAT ?=
SENSITIVITY ?= 15
SILENT ?= false

compare:
	$(RUN_CMD) redline-assist compare --sensitivity $(SENSITIVITY) $(THIS) $(THIS)

