CONFIG ?= config.toml

TARGET_DIR = target/release
REDLINE=$(TARGET_DIR)/redline
REDLINE_ASSIST=$(TARGET_DIR)/redline-assist
export RUST_LOG = info

remove-artifacts:
	@-rm $(REDLINE) $(REDLINE_ASSIST)

build: remove-artifacts $(REDLINE) $(REDLINE_ASSIST)

$(REDLINE) $(REDLINE_ASSIST):
	@cargo build --release --bins

bench: $(REDLINE)
	@$(REDLINE) $(CONFIG)

OUTPUT ?=

report: $(REDLINE_ASSIST)
	@$(REDLINE_ASSIST) report $(OUTPUT)

bench-report: bench report

prepare: $(REDLINE_ASSIST)
	@$(REDLINE_ASSIST) prepare $(CONFIG)

clean: $(REDLINE_ASSIST)
	@$(REDLINE_ASSIST) cleanup

clean-all: $(REDLINE_ASSIST)
	@$(REDLINE_ASSIST) cleanup -a 

THIS ?=
THAT ?=
SENSITIVITY ?= 15
SILENT ?= false

define compare_command
	@$(REDLINE_ASSIST) compare --sensitivity $(SENSITIVITY) $(THIS) $(THAT)
endef

compare: $(REDLINE_ASSIST)
	$(compare_command)

compare-ignore-error:
	-$(compare_command)

bench-compare: bench compare-ignore-error clean

