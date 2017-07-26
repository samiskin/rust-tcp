CARGO := $(shell command -v cargo 2> /dev/null)

all: 
ifndef CARGO
	$(error "Cargo not available, please run on eceLinux 1, 2, or 3")
endif
	cargo build --release;
	cp ./target/release/server ./pa2-358s17;

clean:
	rm ./pa2-358s17;
