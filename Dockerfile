FROM rust:1.80.1 AS build

WORKDIR /app

COPY . .

RUN cargo build --release

RUN cp ./target/release/cris_spam_bot /app/cris_spam_bot

FROM debian:latest AS final

RUN apt update && apt install -y sqlite3 libsqlite3-dev libssl-dev

COPY --from=build /app/cris_spam_bot /bin/

ENTRYPOINT [ "/bin/cris_spam_bot" ]
