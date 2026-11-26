# Marco A — revisão visual do painel Elzevir

A visão geral executável reúne estado do serviço, proteção das cópias, acesso e atualizações. A decisão visual pendente é a aprovação deste dashboard e de seus controles antes de iniciar o Marco B.

## Capturas da execução

| O que revisar | Abrir imagem |
| --- | --- |
| Dashboard completo e comparação com a proposta principal | [Execução em 160×45](dashboard-160x45.png) · [Proposta e execução lado a lado](comparacao-proposta.png) |
| Densidade e hierarquia frente às duas pranchas conceituais e às três telas rejeitadas | [Comparação visual](comparacao-referencias.png) |
| Leitura em janela intermediária | [120×30](dashboard-120x30.png) |
| Ações fixas, rolagem e endereços na janela compacta | [80×24 ao abrir](dashboard-80x24.png) · [Após rolar](dashboard-80x24-acesso.png) |
| Abertura da instalação vazia e confirmação de backup | [Instalação nova](instalacao-nova.png) · [Diálogo aberto pelo mouse](dialogo-backup.png) |
| Estados do componente usado nas telas, na galeria de demonstração | [Foco e hover](galeria-foco-hover.png) · [Botão pressionado](galeria-pressionado.png) · [Resultado de erro simulado](galeria-retorno.png) |

As capturas foram obtidas do binário Linux com uma biblioteca temporária. O ensaio abriu um pseudoterminal, enviou eventos de mouse SGR ao Crossterm, leu a saída ANSI e rasterizou as células com pyte, Pillow e DejaVu Sans Mono. As imagens identificadas como proposta, inspiração e resultado anterior nas comparações vieram do anexo. As cores e a proporção das células podem variar no emulador de terminal do operador.

A grade larga mantém os quatro cartões de estado no topo, as métricas e ações de serviço no centro e os acessos logo abaixo. A tabela inclui seis destinos para distinguir catálogo e equipe no computador, na rede e na internet. Em 120×30 a roda ou o botão **Descer** revela o restante da tabela. Em 80×24 os cartões passam a duas colunas, os painéis centrais se empilham e os atalhos permanecem visíveis.

## O que mudou

| Área | Implementação e evidência |
| --- | --- |
| Contratos reutilizados | `Snapshot`, `Operations`, `Form` e `Worker` continuam responsáveis por observações, confirmação e entrega ao canal administrativo. O teste `dashboard_reports_a_published_backup_even_after_an_export_failure` verifica os estados independentes de cópia local, pedido, exportação e nuvem. |
| Controle compartilhado | `ui::Button` desenha e registra a mesma superfície. `ui::Scene` captura pressionamento, confere a soltura, gerencia foco e invalida alvos que saem da tela. O diálogo usa o mesmo botão nas opções, na confirmação e no cancelamento. Os testes de interação ficam em `ui/mod.rs` e `runtime.rs`. |
| Dashboard | `overview.rs` organiza as medidas e as ações usando a geometria visível. A única nova medida do coletor é a capacidade do sistema de arquivos. A cor de proteção depende da cópia catalogada. A cor de HTTPS depende do teste da rota pública. |
| Operações seguintes | Os atalhos abrem os fluxos vigentes. A central de backups, o explorador, os assistentes de restauração e Cloudflare e a reformulação completa do primeiro acesso seguem nos respectivos marcos. |

## Percurso verificado

1. Abrir e sair de uma instalação nova preservou a ausência do banco. O ensaio criou então uma instituição fictícia e iniciou um servidor em loopback com porta dinâmica.
2. Pressionar **Criar backup** e soltar fora cancelou o gesto. Um clique completo abriu o diálogo. O clique na área de fundo ficou contido pelo modal. Depois de reduzir de 160×45 para 80×24, **Cancelar** fechou o diálogo e o histórico continuou vazio.
3. Dois cliques, um para abrir e outro para confirmar, produziram um único trabalho concluído. A CLI verificou o arquivo cifrado com a chave de recuperação exportada para fora da pasta da instituição.
4. Em 80×24, a roda e **Descer** alcançaram os endereços. **Copiar** emitiu OSC 52 com a URL observada. **Testar acessos locais** confirmou catálogo e equipe por HTTP. O navegador estava indisponível nesse ambiente. A gravação efetiva na área de transferência depende de o terminal aceitar OSC 52.
5. No painel em inglês e modo ASCII, o ensaio colou uma entrada longa com acento, verificou seu sufixo, reduziu o terminal abaixo do mínimo com o diálogo aberto e cancelou ao voltar a 80×24. Na galeria, conferiu bloqueio dos controles indisponível e ocupado, ativação por Tab e Espaço e retorno de uma ação simulada.

O ensaio foi local em Linux. Abertura de navegador gráfico, interação nativa no Windows e validação pública por DNS/HTTPS ficam fora dessas evidências. As medições vêm do host visível ao processo. O painel indica o tamanho observado do banco. O coletor atual não fornece tamanhos separados do índice e dos objetos digitais.

## Reprodução

Na raiz do repositório, com a toolchain do projeto e um terminal interativo, estes comandos abrem o painel da instalação escolhida e a galeria. Substitua o caminho da configuração pela instalação que deseja observar.

```sh
cargo build -p elzevir --locked
target/debug/elzevir panel --config /caminho/da/biblioteca/server.toml --lang pt-BR
target/debug/elzevir panel --demo --lang pt-BR
```

Na demonstração, clique em **Galeria de controles**. Para repetir a prova automatizada em Linux, use um Python com os pacotes `pyte` e `Pillow` e a fonte DejaVu Sans Mono. O script cria a própria instalação temporária e aceita um diretório de saída para as capturas.

```sh
python3 tools/panel/capture_dashboard.py --output /tmp/elzevir-marco-a
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo xtask ci
cargo nextest run --workspace --locked --run-ignored all
cargo deny check
```

## Resultado desta execução

O Nextest executou **1.085 testes**, todos aprovados, incluindo os marcados como opcionais. O ensaio PTY passou nas duas traduções. A regressão `public_staff_uses_the_advertised_staff_path_and_respects_remote_opt_out` cobre a URL pública com o caminho `/equipe` e o bloqueio dos controles quando o broker informa acesso remoto da equipe desativado.

Formatação, Clippy e `xtask ci` passaram. As verificações de dependências do `cargo deny` passaram para advisories, versões proibidas, licenças e origens. A ferramenta reportou avisos de versões transitivas duplicadas e exceções de advisories sem correspondência. O Cargo também registrou um aviso de compatibilidade futura da dependência transitiva `proc-macro-error2`.
