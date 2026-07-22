#!/usr/bin/env python3
"""
Espresso CLI — Host-side tooling for Espresso OS (CLAUDE.md spec)
Speaks the Espresso Host Protocol over UART0 (COM3 / /dev/ttyUSB0).

Usage:
    python espresso.py ps [--port COM3]
    python espresso.py devices [--port COM3]
    python espresso.py pal [--port COM3]
    python espresso.py logs [--port COM3]
    python espresso.py cat /proc/mem [--port COM3]
    python espresso.py cat /dev/hcsr04 [--port COM3]
    python espresso.py ping [--port COM3]
    python espresso.py deploy app_name [role=pin ...] [--pins 12,13] [--port COM3]
    python espresso.py remove app_name [--port COM3]

Examples:
    python espresso.py deploy hcsr04 trigger=12 echo=13 --port COM3
    python espresso.py deploy motor_app pwm=32 dir1=33 dir2=25 --port COM3
    python espresso.py deploy servo_app signal=27 --port COM3
"""

import sys
import time
import struct
import argparse

try:
    import serial
except ImportError:
    print("Error: 'pyserial' package is required. Install it using: pip install pyserial")
    sys.exit(1)

MAGIC_REQ  = b'\x1bS'
MAGIC_RESP = b'\x1bR'

CMD_PS      = 0x01
CMD_DEVICES = 0x02
CMD_LOGS    = 0x03
CMD_CAT     = 0x04
CMD_DEPLOY  = 0x05
CMD_REMOVE  = 0x06
CMD_PAL     = 0x07
CMD_PING    = 0x08

def build_frame(cmd: int, seq: int = 0, payload: bytes = b"") -> bytes:
    length = len(payload)
    return MAGIC_REQ + bytes([cmd, seq, (length >> 8) & 0xFF, length & 0xFF]) + payload

def sync_read_header(ser, timeout_sec: float = 2.0) -> bytes:
    start_time = time.time()
    buf = bytearray()
    while time.time() - start_time < timeout_sec:
        b = ser.read(1)
        if not b:
            continue
        buf.append(b[0])
        if len(buf) >= 2 and buf[-2:] == MAGIC_RESP:
            rest = ser.read(5)
            if len(rest) < 5:
                raise TimeoutError("Incomplete response header after magic.")
            return bytes(buf[-2:]) + rest
    raise TimeoutError(f"Timed out waiting for magic header. Stream snippet: {bytes(buf[:60])}")

def send_recv(port: str, cmd: int, payload: bytes = b"", baudrate: int = 115200, timeout: float = 2.0) -> bytes:
    try:
        ser = serial.Serial()
        ser.port = port
        ser.baudrate = baudrate
        ser.timeout = timeout
        ser.dtr = False
        ser.rts = False
        ser.open()

        with ser:
            ser.reset_input_buffer()

            frame = build_frame(cmd, 0, payload)
            ser.write(frame)
            ser.flush()

            resp_header = sync_read_header(ser, timeout_sec=timeout)

            resp_cmd = resp_header[2]
            seq = resp_header[3]
            status = resp_header[4]
            len_hi = resp_header[5]
            len_lo = resp_header[6]

            payload_len = (len_hi << 8) | len_lo
            resp_payload = ser.read(payload_len)

            if status != 0:
                print(f"Device returned status error: {status}", file=sys.stderr)

            return resp_payload
    except Exception as e:
        print(f"Serial communication error on {port}: {e}", file=sys.stderr)
        sys.exit(1)

def main():
    parser = argparse.ArgumentParser(description="Espresso OS Host CLI Tool")
    parser.add_argument("command", choices=["ps", "devices", "pal", "logs", "cat", "ping", "deploy", "remove"], help="Host command")
    parser.add_argument("target", nargs="?", default="", help="App name, device path, or target")
    parser.add_argument("extra_args", nargs="*", default=[], help="Extra role parameters (e.g. trigger=12 echo=13)")
    parser.add_argument("--pins", "-pin", default="", help="User-specified GPIO pins (e.g. --pins 12,13 or --pins 32,33,25)")
    parser.add_argument("--port", "-p", default="COM3", help="Serial port (default: COM3)")

    args = parser.parse_args()

    cmd_map = {
        "ps": CMD_PS,
        "devices": CMD_DEVICES,
        "pal": CMD_PAL,
        "logs": CMD_LOGS,
        "cat": CMD_CAT,
        "ping": CMD_PING,
        "deploy": CMD_DEPLOY,
        "remove": CMD_REMOVE,
    }

    cmd_id = cmd_map[args.command]

    if args.command == "deploy":
        parts = [args.target]
        if args.extra_args:
            parts.extend(args.extra_args)
        if args.pins:
            parts.append(f"pins={args.pins}")
        payload_str = " ".join(parts)
        payload = payload_str.encode("utf-8")
    else:
        payload = args.target.encode("utf-8") if args.target else b""

    result = send_recv(args.port, cmd_id, payload)

    # Format binary device frames vs text proc endpoints
    if args.command == "cat" and args.target.startswith("/dev/"):
        if len(result) == 4:
            distance_mm = struct.unpack("<I", result)[0]
            print(f"{distance_mm} mm (DistanceSensor binary frame: 0x{result.hex()})")
        else:
            print(f"Device binary frame ({len(result)} bytes): 0x{result.hex()}")
    else:
        print(result.decode("utf-8", errors="replace"), end="")

if __name__ == "__main__":
    main()
