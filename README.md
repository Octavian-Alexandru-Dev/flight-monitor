# Flight Monitor

Web app personale che controlla periodicamente i prezzi dei voli su ricerche salvate e segnala le occasioni su Telegram. Non prenota: mostra il volo e il link al sito dove comprarlo.

La differenza rispetto ai servizi esistenti è la flessibilità combinata: più aeroporti di partenza, destinazioni per città, paese, raggio o "ovunque", scali con durata minima e massima, date flessibili, open-jaw, biglietti separati via hub, viaggi di gruppo, tutto monitorato in automatico con storico prezzi.

Gira su una macchina di casa (PC, mini PC, Raspberry Pi), senza servizi a pagamento.

## Avvio rapido

Requisiti: Rust 1.88 o successivo. Nient'altro: è un unico binario.

```sh
cp .env.example .env                               # opzionale: Telegram, Travelpayouts, LLM locale
cargo build --release
./target/release/flight-monitor install-browser    # trova Chrome o scarica Chromium (una volta)
./target/release/flight-monitor                    # scheduler + interfaccia su http://127.0.0.1:8080
```

Le ricerche si creano dall'interfaccia web. Il primo controllo parte appena la ricerca è salvata.

## Comandi

| Comando | Cosa fa |
|---|---|
| `flight-monitor serve` | Scheduler e interfaccia web (predefinito). |
| `flight-monitor search --from FCO,CIA --to NRT --date 2026-12-10 [--return 2026-12-20]` | Una ricerca live singola, stampa i risultati normalizzati. |
| `flight-monitor seed examples/searches.json` | Salva ricerche da JSON e stampa la stima di query per ciclo. |
| `flight-monitor run-once [--all] [--search ID]` | Esegue subito le ricerche dovute, tutte o una. |
| `flight-monitor summary [--send]` | Stampa (e invia) il riepilogo giornaliero. |
| `flight-monitor install-browser` | Trova Chrome o scarica Chromium in `data/chromium`. |
| `flight-monitor import-geo [--sources DIR]` | Ricostruisce i dataset di aeroporti, paesi e compagnie. |

## Configurazione

Tutto tramite variabili d'ambiente o file `.env` (vedi `.env.example`). Ogni variabile ha anche un flag equivalente (`--help`).

| Variabile | Predefinito | Note |
|---|---|---|
| `FM_DATABASE_URL` | `sqlite://data/flight-monitor.db` | SQLite, creato al primo avvio. |
| `FM_BIND` | `127.0.0.1:8080` | Usa `0.0.0.0:8080` per raggiungerla dalla rete di casa. |
| `FM_PUBLIC_URL` | | Base dei link "Dettagli" nei messaggi Telegram. |
| `FM_CHROME` | | Chrome o Chromium da usare. Se assente: Chrome installato, altrimenti Chromium scaricato in `data/chromium`. |
| `FM_GOOGLE_MIN_INTERVAL_SECS` | `20` | Pausa minima tra due richieste a Google Flights. Valori bassi portano al blocco "unusual traffic". |
| `TELEGRAM_BOT_TOKEN`, `TELEGRAM_CHAT_ID` | | Senza, gli alert restano nell'interfaccia e nei log. |
| `TRAVELPAYOUTS_TOKEN` | | Abilita la fase di scan e le destinazioni "Ovunque". |
| `FM_CACHE_TTL_MINUTES` | `60` | Query identiche più recenti vengono servite dal database. |
| `FM_VERIFY_TOP_N` | `8` | Query live verificate per gruppo dopo uno scan. |
| `FM_DEFAULT_BUDGET`, `FM_DEFAULT_POLL_HOURS` | `40`, `6` | Valori iniziali del modulo. |
| `FM_DAILY_SUMMARY_HOUR` | | Ora locale del riepilogo Telegram. |
| `FM_RETENTION_DAYS` | `120` | Le osservazioni grezze più vecchie vengono eliminate; lo storico delle ricerche resta. |
| `FM_LLM_URL`, `FM_LLM_MODEL` | | Endpoint OpenAI-compatibile locale (es. Ollama) per le ricerche in linguaggio naturale. |

**Telegram.** Crea un bot con @BotFather, scrivigli un messaggio e leggi il tuo `chat.id` da `https://api.telegram.org/bot<TOKEN>/getUpdates`.

**Travelpayouts.** Il token gratuito si ottiene registrandosi su travelpayouts.com (sezione API).

**LLM locale.** Con Ollama: `ollama pull llama3.1`, poi `FM_LLM_URL=http://localhost:11434/v1/chat/completions`. Nella pagina "Nuova ricerca" compare il campo per descrivere la ricerca a parole.

## Deploy su una macchina di casa

Su Raspberry Pi (Linux ARM) Chromium non si può scaricare automaticamente: installalo con `sudo apt install chromium` e imposta `FM_CHROME=/usr/bin/chromium`.

```sh
cargo build --release
sudo cp deploy/flight-monitor.service /etc/systemd/system/   # adatta utente e percorsi
sudo systemctl enable --now flight-monitor
```

L'IP residenziale è voluto: riduce i blocchi anti-bot e rende inutili i proxy. Il polling è nell'ordine delle ore, con pause tra le richieste a ogni fonte.

## Funzionalità

**Ricerche.** Insiemi di luoghi (aeroporto, lista, città con tutti i suoi scali, paese, raggio in km, ovunque), solo andata, A/R abbinato (N × M combinazioni) o A/R libero (N + M, compagnie e aeroporti diversi tra andata e ritorno), finestre di date e durata del soggiorno.

**Vincoli.** Scali massimi, durata scalo minima e massima, durata totale, compagnie escluse, fasce orarie di partenza escluse, bagagli in stiva inclusi nel prezzo, prezzo massimo.

**Monitoraggio.** Scheduler interno, budget di query per ciclo con priorità alle combinazioni storicamente più convenienti e rotazione delle altre, deduplica delle query identiche tra ricerche, cache con TTL, frequenza adattiva (più spesso vicino alla partenza o con prezzi instabili, meno se lontano e stabile).

**Scan e verifica.** Con un token Travelpayouts, i prezzi in cache sfoltiscono le combinazioni e solo le migliori vengono verificate live su Google Flights.

**Alert Telegram.** Prezzo sotto soglia, calo percentuale, nuovo minimo storico, prezzo anomalo (oltre due deviazioni standard sotto la media: messaggio prioritario). Mai due volte lo stesso volo allo stesso prezzo. Riepilogo giornaliero.

**Affidabilità delle fonti.** Errori di parsing, blocchi o silenzi sospetti (nessun risultato su una rotta che di solito ne ha) degradano la fonte e inviano un avviso: un silenzio non viene mai scambiato per "nessun volo".

**Oltre l'MVP.** Open-jaw, biglietti separati via hub con avvisi di rischio (nessuna protezione sulla coincidenza, cambio aeroporto nella stessa città, scalo notturno), stopover voluti, costo porta a porta (trasferimenti per aeroporto e pernottamenti), viaggi di gruppo da città diverse (costo totale minimo o arrivi allineati), destinazione aperta, calendario dei prezzi, consiglio "compra ora / aspetta" basato sullo storico, ricerche in linguaggio naturale con LLM locale.

## Architettura

```
crates/
  domain/     tipi del dominio, nessun I/O
  geo/        dataset di riferimento e risoluzione dei luoghi in aeroporti IATA
  providers/  trait FlightProvider, Google Flights (HTTP + Chromium headless), Travelpayouts
  planner/    ricerca → query, dedup, budget, scan → verify, filtri, combinazioni, polling adattivo
  storage/    SQLite con sqlx (query verificate a compile time), migrazioni
  alerts/     regole, messaggi, notifica Telegram
  web/        motore, scheduler, CLI, Axum + Askama + htmx
fixtures/     risposte registrate per i test (vedi fixtures/README.md)
data/         aeroporti (OurAirports), paesi, compagnie (OpenFlights), città con più aeroporti
```

Tutto è in Rust. Google Flights include i risultati nella pagina solo per le ricerche che ha già in cache; per le altre li carica dopo con una chiamata firmata dal suo JavaScript anti-bot. Il provider fa prima una richiesta HTTP semplice e, se la pagina non contiene voli, la apre in Chromium headless (`chromiumoxide`) e intercetta la risposta. La query (`tfs`) è un protobuf codificato a mano.

Una sola query a Google Flights può coprire fino a 7 aeroporti per lato: la ricerca "Calabria → Tokyo" su 3 date costa 3 query, non 18.

## Sviluppo

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
```

Le query SQL sono verificate a compile time anche offline grazie a `.sqlx/`. Dopo aver cambiato una query: `./scripts/sqlx-prepare.sh` (richiede `cargo install sqlx-cli --no-default-features --features sqlite,rustls`).

Altri script: `scripts/build-icons.py` rigenera il sottoinsieme di icone Phosphor usato dall'interfaccia.

## Limiti noti

- Google Flights non ha un'API pubblica: se il sito cambia formato, la fonte viene segnalata come degradata e va aggiornato il parser (`crates/providers/src/google_flights.rs`). Le fixture permettono di verificarlo senza rete.
- Gli orari sono locali agli aeroporti; le durate di scali e viaggi sono calcolate dai segmenti.
- Le fixture Travelpayouts e LLM sono costruite dal formato documentato, non registrate: vanno sostituite con risposte reali appena disponibili un token e un modello locale.
- Scraper per singole compagnie: non presenti; da valutare solo per le rotte in cui Google Flights non basta.
- I termini d'uso di quasi tutte le fonti vietano lo scraping. Per uso personale a basso volume il rischio è minimo; va rivalutato in caso di pubblicazione.
