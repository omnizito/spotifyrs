# SpotifyRS — protótipo 0.1

Companion desktop experimental em **Rust + Slint**. A interface abre em inglês quando ainda não há uma preferência guardada. Para seguir este guia com os nomes dos menus em português, escolhe **Settings → App language → Português (Portugal)**. Interface nativa, sem Electron, Tauri, Svelte, JavaScript ou WebView incorporada. O nome SpotifyRS é provisório e interno: a política de marca do Spotify não permite nomes públicos que comecem por «Spot» sem permissão aplicável.

## Experimentar

Abre o SpotifyRS e escolhe **Experimentar demonstração**. Podes abrir três playlists de exemplo, pesquisar seis faixas fictícias, consultar a fila e testar os controlos. Este modo é silencioso e funciona sem credenciais.

Para ouvir música neste computador, liga o Soloist e escolhe uma faixa da fila, cola um link ou pesquisa pelo nome depois de autorizar o catálogo. O [guia do Soloist](SOLOIST.md) explica a configuração.

## Ligar a conta Premium

1. Abre o [Spotify for Developers](https://developer.spotify.com/dashboard) e cria uma aplicação, se essa opção estiver disponível na tua conta. Escolhe um nome público que cumpra as regras de marca, por exemplo um nome próprio independente de «Spotify» ou «Spot».
2. Seleciona **Web API** e regista exatamente este Redirect URI: `http://127.0.0.1:8888/callback`. Usa o endereço IP, não `localhost`.
3. Adiciona a tua conta à lista de utilizadores autorizados da aplicação, se necessário. O proprietário precisa de Premium; o modo de desenvolvimento permite até cinco utilizadores autorizados nas novas aplicações.
4. Copia o **Client ID** e cola-o em **Ligações**. Escolhe **Autorizar biblioteca** para veres as tuas playlists, faixas guardadas e pesquisa. **Ligar via Web API** permite controlar outro dispositivo Spotify.
5. Autoriza no navegador oficial. A palavra-passe nunca é pedida pelo protótipo. Podes cancelar na janela; o pedido expira após três minutos.
6. No modo Web API, inicia uma faixa num computador, telemóvel ou coluna compatível e escolhe **Atualizar estado** em **Ligações**.
7. Usa os controlos ou escolhe um dispositivo e carrega em **Usar dispositivo**. A transferência não solicita o início da reprodução; podes carregar em Reproduzir depois.

O SpotifyRS guarda o Client ID ao fechar. Também podes fornecê-lo com a variável de ambiente `SPOTIFY_CLIENT_ID`. Os tokens de renovação são guardados no Gestor de Credenciais do Windows ou no cofre Secret Service no Linux. A pesquisa recupera a autorização ao abrir e **Ligar via Web API** reutiliza a autorização de reprodução, se existir. **Esquecer autorização** remove as autorizações guardadas neste PC. Para revogar o acesso na conta, usa a [página de aplicações do Spotify](https://www.spotify.com/account/apps/).

## O que está incluído

- Login OAuth Authorization Code com PKCE S256, `state` aleatório e retorno apenas em loopback.
- Leitura do estado de reprodução e dispositivos pela Web API oficial.
- Reprodução/pausa, anterior/seguinte, transferência para dispositivo e ligação ao Spotify.
- Verificação de dispositivos restritos e ações não permitidas pela API.
- Tratamento de sessão expirada, ausência de dispositivo, falhas de rede, HTTP 403/429 e quota esgotada.
- Espera de `Retry-After`, atualizações espaçadas e nenhuma repetição automática de comandos de mudança de faixa.
- Interface com tema escuro e demonstração sem credenciais.

## Limites deste protótipo

O áudio local é reproduzido pelo processo oficial Soloist. O modo Web API controla um dispositivo Spotify Connect. A interface Slint mantém-se nativa em ambos os modos.

A API de playlists permite listar as faixas das playlists que criaste ou em que colaboras. Podes abrir as restantes com **Abrir no Spotify**. [Referência de playlists](https://developer.spotify.com/documentation/web-api/reference/get-playlists-items)

Se o dispositivo for o Spotify desktop no mesmo computador, o consumo total inclui também o processo do Spotify. Medir apenas o companion não demonstra uma redução do consumo total nessa situação.

O acesso real depende das permissões, disponibilidade e quotas da aplicação Spotify. Usar endpoints oficiais não garante aprovação do produto. O âmbito é uma ferramenta complementar compacta; uma substituição da experiência principal do Spotify exige análise e possível autorização escrita. Não existe lançamento público, distribuição comercial ou garantia de conformidade jurídica nesta entrega.

A pesquisa mostra até dez faixas por consulta. A interface inclui a fila local do Soloist, a reprodução por link e a barra de controlos. Biblioteca pessoal, playlists, capas, volume e avanço para uma posição específica ficam para outra iteração.

## Instalar no Arch Linux

Num sistema Arch Linux x86_64 atualizado, instala `base-devel` com `sudo pacman -S --needed base-devel`. Na raiz do checkout, executa como utilizador normal:

```sh
cd outputs/SpotifyRS/packaging/arch
makepkg --syncdeps --install
spotifyrs --demo
```

O pacote compila com as versões fixadas em `Cargo.lock`, executa os testes e instala o comando `spotifyrs` e a entrada **SpotifyRS** no menu da sessão X11 ou Wayland. Para desenvolvimento, executa `cargo test --locked` e `cargo run --locked -- --demo` em `outputs/SpotifyRS`.

As preferências ficam em `${XDG_CONFIG_HOME:-~/.config}/spotifyrs/settings.json`. Para recuperar autorizações entre sessões ou guardar a chave do Soloist, usa um cofre que disponibilize Secret Service na sessão D-Bus, com um cofre predefinido desbloqueado. Por exemplo, instala `gnome-keyring` e inicia sessão com o cofre configurado. Sem cofre podes usar a demonstração e autorizar uma sessão, mas terás de voltar a autorizar após fechar a aplicação.

## Compilar e testar no Windows

Instala [Rust](https://www.rust-lang.org/tools/install). No Windows, a configuração habitual é a toolchain MSVC com Visual Studio Build Tools e Windows SDK; a toolchain GNU também é suportada com as ferramentas de ligação adequadas. As dependências estão fixadas em `Cargo.lock`.

```powershell
cargo test --locked
cargo build --release --locked
cargo run --release --locked -- --demo
```

Para este ambiente foi criada uma toolchain isolada na pasta `work`, fora do projeto entregue. O script `work/build.ps1` da pasta de trabalho permite reproduzir os testes e a compilação local sem alterar o PATH do Windows. Essa ferramenta de desenvolvimento não faz parte do pacote portátil.

## Medir recursos

```powershell
./scripts/Measure-Performance.ps1 -Executable ./spotifyrs.exe -Scenario demo -Seconds 60
```

O script abre uma instância, mede memória residente (Working Set), memória privada (Private Bytes), pico residente e CPU normalizada pelo número de processadores lógicos. Grava dados brutos CSV e um resumo JSON em `measurements`. Fecha apenas a instância que criou. Usa `-ProcessId` para observar uma sessão já aberta; nesse caso não fecha a aplicação.

O objetivo exploratório é ficar abaixo de 150 MiB de memória residente em uso normal. **Não é uma garantia nem uma comparação com outras GUIs.** Consulta `VALIDACAO.md` para resultados efetivamente obtidos e testes ainda pendentes. Os testes de desempenho medem o processo local e usam dados fictícios, sem analisar hábitos de audição ou conteúdo Spotify.

## Estrutura

| Ficheiro | Responsabilidade |
| --- | --- |
| `ui/app.slint` | Interface declarativa nativa |
| `src/main.rs` | Ligações entre UI, comandos e estado |
| `src/worker.rs` | Estado da sessão, fila limitada e trabalho em segundo plano |
| `src/auth.rs` | PKCE, retorno local, troca e renovação de tokens |
| `src/api.rs` | Cliente HTTP, endpoints, limites e erros |
| `src/model.rs` | Modelos de dados, permissões e demonstração |
| `scripts/Measure-Performance.ps1` | Medição do processo no Windows |

A rede é executada fora da thread da interface. A fila admite quatro comandos; o estado visual é atualizado pelo event loop do Slint. O renderer por software evita trazer motores web ou um renderer GPU adicional. Isto é uma escolha inicial a medir, não prova que seja a opção mais eficiente em todo o hardware. Respostas da API são limitadas a 1 MiB e a lista local a 32 dispositivos. Não há base de dados, cache de áudio ou capas.

## Referências oficiais consultadas em 7 de novembro de 2026

- [PKCE](https://developer.spotify.com/documentation/web-api/tutorials/code-pkce-flow)
- [Redirect URIs](https://developer.spotify.com/documentation/web-api/concepts/redirect_uri)
- [Quotas e modo de desenvolvimento](https://developer.spotify.com/documentation/web-api/concepts/quota-modes)
- [Limites de pedidos](https://developer.spotify.com/documentation/web-api/concepts/rate-limits)
- [Transferência de reprodução](https://developer.spotify.com/documentation/web-api/reference/transfer-a-users-playback)
- [Web Playback SDK](https://developer.spotify.com/documentation/web-playback-sdk)
- [Política de desenvolvimento e marca](https://developer.spotify.com/policy)
- [Slint: funcionalidades de compilação](https://docs.slint.dev/latest/docs/rust/slint/docs/cargo_features/)

Slint é usado sob a alternativa [Royalty-free 2.0](https://slint.dev/terms-and-conditions), com a atribuição oficial em **Ajuda > Sobre** e a licença na pasta `licenses`. O inventário de dependências e avisos está em `THIRD_PARTY_NOTICES.md`. Rever o conjunto antes de eventual distribuição pública; esta entrega é um protótipo local sem publicação.

