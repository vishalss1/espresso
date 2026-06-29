# Makefile for edit program - line editor / source code editor
# 
# Compiles edit.c to native Xtensa code and converts to .espr format

CC = xtensa-esp32-elf-gcc
PYTHON = python3

# Source files
SRC = edit.c

# Target files
OBJ = $(SRC:.c=.o)
ESPR = edit.espr

# Build target
all: $(ESPR)

# Compile C source to object file
%.o: %.c
	@echo "Compiling $< for Xtensa..."
	$(CC) -nostdlib -I../.. -mno-red-zone -ffreestanding -nostartfiles -c $< -o $@

# Convert object to .espr format using SDK tool
$(ESPR): $(OBJ)
	@echo "Converting $< to .espr format..."
	$(PYTHON) ../../sdk/emit_espr.py $< $@

# Clean up build artifacts
clean:
	rm -f *.o *.espr

.PHONY: all clean