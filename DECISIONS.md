# Transport - QUIC
- wiele równoległych streamów
- utrzymanie połączenia przy zmianie źródła internetu
- dobra okazja do wypróbowania

# Odnajdywnaie node'ów - Bootstrap peers i peers exchange
- w mojej sieci będzie jeden node o statycznym ip, który pomoże nawiązać pierwotne połaczenie
- peers exchange pozwoli na odnalezienie większej ilości nodów

# Potwierdzenie tożsamości
- tworzenie kluczy Ed25519
- id na podstawie klucza publicznego

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
