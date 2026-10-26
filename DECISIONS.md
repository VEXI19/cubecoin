# Transport - QUIC
- wiele równoległych streamów
- utrzymanie połączenia przy zmianie źródła internetu
- dobra okazja do wypróbowania

# Odnajdywnaie node'ów - Bootstrap peers i peers exchange
- w mojej sieci będzie jeden node o statycznym ip, który pomoże nawiązać pierwotne połaczenie
- peers exchange pozwoli na odnalezienie większej ilości nodów

# Potwierdzenie tożsamości
- tworzenie kluczy Ed25519
    - Nonce generation
    - Signature malleability
    - Curve point encoding
    - Low-s normalization
    - Public-key recovery conventions
- address = bech32m(sha256(public_key)) with address version
- id na podstawie klucza publicznego

# Persistent wallet identity (Linux only)
- The wallet stores exactly one encrypted Ed25519 seed at `$XDG_DATA_HOME/cubecoin/identity/wallet.bin`, falling back to `~/.local/share/cubecoin/identity/wallet.bin`.
- Development builds may use an absolute `CUBECOIN_WALLET_DEV_PATH` override; release builds reject it.
- File format v1 is a fixed 132-byte binary format with `CUBEWALT` magic and an authenticated 84-byte header.
- Argon2id v19 derives a 32-byte key using 64 MiB memory, 3 iterations, 1 lane, and a fresh 32-byte salt.
- XChaCha20-Poly1305 uses a fresh 24-byte nonce to encrypt the 32-byte seed; the header is AEAD additional authenticated data.
- The wallet file is created atomically without overwriting an existing file. Linux directories use mode `0700`; the wallet file uses mode `0600`.
- Password input is raw UTF-8 without trimming or normalization. Empty passwords are allowed by explicit user choice and are insecure.
- Loss of either the encrypted wallet file or password permanently loses access. No recovery, password change, or seed export exists yet.

# Format wiadomości - ProtoBuff
- schemas
- automatyczne generowanie kodu ze schematów
- serializacja binarna

# Wiele streamów - Long-lived control stream
- jeden stream do pingowania i wymiany peer ip's
- każda inna wymiana danych tworzy nowy stream

# Baza danych - SQLite
- trzeba przetrzymywać tylko parę istotnych informacji
    - klucze
    - adresy znanych nodeów
    - wcześniej zweryfikowane node'y



# ilość połączeń
- standard 8
- max 24
