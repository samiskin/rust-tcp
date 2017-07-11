
all: 
	cargo build;
	cp ./target/debug/ece358 ./pa2-358s17;

clean:
	rm ./pa2-358s17;
