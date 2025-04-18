SRC := $(find bencher assist core -type f -name '*.rs')

CONFIG ?= config.toml

TARGET_DIR = target/release
REDLINE=$(TARGET_DIR)/redline
REDLINE_ASSIST=$(TARGET_DIR)/redline-assist


build: $(REDLINE) $(REDLINE_ASSIST)

$(REDLINE) $(REDLINE_ASSIST): $(SRC)
	@cargo build --release --bins

bench: $(REDLINE)
	@$(REDLINE) $(CONFIG)

OUTPUT ?=

report: $(REDLINE_ASSIST)
	@$(REDLINE_ASSIST) report $(OUTPUT)

prepare: $(REDLINE_ASSIST)
	@$(REDLINE_ASSIST) prepare $(CONFIG)

cleanup: $(REDLINE_ASSIST)
	@$(REDLINE_ASSIST) cleanup

clean-all: $(REDLINE_ASSIST)
	@$(REDLINE_ASSIST) cleanup -a 

THIS ?=
THAT ?=
SENSITIVITY ?= 15
SILENT ?= false

compare: $(REDLINE_ASSIST)
	@$(REDLINE_ASSIST) compare --sensitivity $(SENSITIVITY) $(THIS) $(THIS)

