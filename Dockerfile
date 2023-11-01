FROM golang:1.21-alpine
WORKDIR /niketsu-server
COPY server/ ./server
COPY LICENSE .
COPY go.mod .
COPY go.sum .

RUN go build -o /bin/niketsu-server server/main.go

ARG HOST="0.0.0.0"
ARG PORT=7766
ARG CERT=""
ARG KEY=""
ARG PASSWORD=""
ARG DBPATH=".db"
ARG DBUPDATEINTERVAL=10
ARG DBWAITTIMEOUT=4
ARG DEBUG=false

ENV HOST=${HOST}
ENV PORT=${PORT}
ENV CERT=${CERT}
ENV KEY=${KEY}
ENV PASSWORD=${PASSWORD}
ENV DBPATH=${DBPATH}
ENV DBUPDATEINTERVAL=${DBUPDATEINTERVAL}
ENV DBWAITTIMEOUT=${DBWAITTIMEOUT}
ENV DEBUG=${DEBUG}

ENTRYPOINT ["/bin/niketsu-server"]
