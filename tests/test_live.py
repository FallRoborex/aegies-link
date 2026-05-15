import socket
import struct
import hmac
import hashlib
import os

sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
sock.settimeout(5)
addr = ("18.216.212.141", 8080)

SECRET = b"testing_pretty_cool"

# Step 1 - Send connect packet
print("-- Connecting --")
header = struct.pack(">BI", 0, 0)
sock.sendto(header, addr)

# Step 2 - Receive auth challenge (nonce)
response, _ = sock.recvfrom(1024)
print(f"Got packet type: {response[0]}")
nonce = response[6:]
print(f"Got nonce: {nonce.hex()}")

# Step 3 - Send HMAC response
sig = hmac.new(SECRET, nonce, hashlib.sha256).digest()
auth_payload = sig
auth_header = struct.pack(">BI", 6, 1)  # AuthResponse type
sock.sendto(auth_header + auth_payload, addr)
print("Sent auth response")

# Step 4 - Receive welcome
try:
    response, _ = sock.recvfrom(1024)
    print(f"Got packet type: {response[0]}")
    payload = response[6:]
    print(f"Server replied: {payload.decode('utf-8', errors='replace')}")
except socket.timeout:
    print("No welcome received - check auth logic")