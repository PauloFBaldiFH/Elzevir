# Painel de controle conectado — P2

A [P3](diagnostico.md) acrescenta verificações explícitas, logs e exportação
na seção Diagnóstico, preservando as observações leves descritas nesta etapa.
A [P4](enderecos-acesso.md) fornece URLs compatíveis com o modo real da equipe
e a seção de endereços com alternativas/testes separados da configuração.

## 1. Comportamento

1. `elzevir panel --config elzevir.toml [--lang pt-BR] [--ascii]` abre a TUI
   conectada; `--data-dir` sobrepõe a pasta do arquivo como na A1. É obrigatório
   fornecer config, data-dir ou `--demo`; demo não aceita config/data-dir.
   A demonstração da P1 permanece independente do servidor e de medições reais.
2. A UI recebe snapshots tipados por uma função fornecida pelo binário. A
   crate do painel não depende de banco, HTTP, IPC ou implementação das operações.
3. Uma thread consulta o mesmo estado da CLI (`Status` por IPC da A4, ou sonda
   A3 legada). A consulta não abre SQLite. Atualiza a cada segundo após concluir
   a leitura anterior. A tecla `f` solicita atualização; pedidos são coalescidos.
4. Há somente uma leitura em andamento e um snapshot pendente. A thread de
   desenho nunca aguarda rede, filesystem ou uma fila cheia. Sair restaura o
   terminal sem esperar por uma leitura ainda bloqueada; o worker recebe sinal
   de parada e não inicia outra consulta. Não há comandos de escrita nesta etapa.
5. O painel de controle mostra estado, versão, PID, segundos de uptime, banco
   e indexador. Estado saudável é verde, degradado amarelo; parado e falha de
   leitura são neutros e distintos. Texto sempre acompanha a cor.
6. Mostra a idade desde o início da coleta, para uma leitura lenta não revalidar
   dados antigos. Sem leitura, aparece consultando; após cinco
   segundos sem observação nova, não mostra saúde antiga como atual. Uma falha
   substitui o resultado anterior e a próxima leitura pode recuperar a tela.
7. CPU e memória são da máquina onde o painel roda, não consumo exclusivo do
   Elzevir. `sysinfo` só fornece CPU após duas amostras com intervalo suficiente;
   erro/valor inválido é desconhecido, nunca zero inventado.
8. Espaço livre, tamanho do arquivo do banco e do WAL são consultas ao
   filesystem (`fs4` e metadata). Não abrem a base. WAL ausente conta como zero
   só se o arquivo do banco existe; erro e arquivo desconhecido não viram zero.
9. O nome da instituição vem da configuração, quando presente. Configurado e
   observado são campos separados: a metadata da execução inclui `listen`
   opcional com IP/porta vinculados. Só após relatório confirmado ele é usado
   como observado. Metadata antiga sem esse campo mostra não lido.
10. O endereço local do catálogo/equipe deriva do listener confirmado;
    bind em 0.0.0.0 ou :: usa loopback para o endereço deste computador.
    Não se afirma teste HTTP, acessibilidade pela LAN ou publicação na internet.
    Essa parte da matriz aparece como não medida/não configurada até P4/P5.
11. As demais seções continuam navegáveis e identificadas como ainda não
    disponíveis no modo conectado. Backup, túnel, atualização e reinício não
    são simulados como operações reais. As ações são entregas posteriores.
12. Todos os textos usam Fluent (en/pt-BR). Valores externos são limitados e
    filtrados antes do terminal para impedir sequências de controle e bidi.

## 2. Origem no Koha

Não porta comportamento do Koha. Base: P2 da análise do painel fornecida por
Paulo, usando os estados observados das etapas A3/A4.

## 3. Paridade

Livre. `sysinfo` passa a medir o host em todas as plataformas suportadas.
`fs4` (MIT/Apache-2.0, já transitivo) consulta espaço sem código unsafe local.
O contrato HTTP/IPC de saúde continua igual; `listen` é extensão opcional da
metadata de descoberta, sem novos comandos administrativos.

## 4. Casos de borda

- Demonstração preserva snapshots; conectado não mostra selo de demonstração.
- Running, degraded, stopped, unavailable e observação antiga são distintos.
- Falha remove valores de saúde anteriores; reconexão recupera a tela.
- Backend bloqueado não prende a saída nem cria fila de atualizações.
- Campos ausentes e CPU inicial ficam desconhecidos; controles externos não
  alcançam o terminal; as duas traduções cobrem o modo conectado.
- Configuração inválida/uso sem TTY não cria arquivos nem emite escapes.
- Painel real acompanha parada/reinício em teste de terminal Linux.

## 5. Desvios

Nenhum: não há área de paridade.

## 6. Fora do escopo

Botões de operação, logs ao vivo/diagnóstico completo (P3), edição e testes de
endereços (P4), backup, serviço, atualização, lista de instituições, histórico
de erros por 24h e contagem de efeitos pendentes. Métricas são instantâneas;
num contêiner, as métricas de host dependem do que o SO disponibiliza.

## Visão geral operacional

A projeção de proteção separa quatro observações: cópia local publicada e verificada na criação, última tentativa do servidor, resultado de exportação pelo operador e transferência do plano de nuvem. Um recibo de exportação antigo não identifica o artefato. O painel informa o resultado como observação independente. Arquivo não catalogado, catálogo ilegível e cópia catalogada ausente impedem afirmar que há proteção verificada. A tela não recalcula hashes a cada coleta. O histórico usa a hora da solicitação. Informação vencida perde a indicação positiva.

Na visão geral, saúde, proteção, recursos e acesso têm cartões próprios. As medidas do processo vêm do PID confirmado e CPU exige duas amostras. Em tela compacta as seções conservam seus nomes e o conteúdo pode rolar sem deslocar as ações.

### Interação da visão geral (Marco A)

O botão registra a área que desenhou. O pressionamento captura o controle, e a soltura na mesma área ativa uma única intenção. Uma mudança de tamanho, diálogo, seção ou posição do controle invalida a captura. Controles indisponíveis ou ocupados não aceitam ativação. Tab e Shift+Tab percorrem as ações disponíveis; Enter ou Espaço as acionam. Campos de texto preservam o espaço como entrada. O diálogo captura a interação até a confirmação ou o cancelamento.

A visão geral mantém navegação e ações rápidas visíveis. Em larguras menores, os grupos se reorganizam e a roda ou os controles de rolagem revelam o conteúdo. A barra de disco usa o espaço disponível e a capacidade observados no mesmo sistema de arquivos. A ausência de medição não representa zero. Um endereço anunciado, um teste HTTP local e um teste HTTPS público são evidências distintas.
