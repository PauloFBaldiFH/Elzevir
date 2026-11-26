# Servidor SIP2 (v0)

> Status: v0, tarefa 4.5. Base: 3M Standard Interchange Protocol 2.00, [empréstimo](emprestimo.md), [devolução](devolucao.md), [renovação](renovacao.md), [leitores](leitores.md) e [conta](conta.md). Critério da tarefa: **cenários SIP2 com as mesmas respostas do oráculo com o SIP ligado**.

Terminais de autoatendimento, cabines de devolução e catracas falam SIP2 com o sistema da biblioteca. O Elzevir responde como o servidor SIP do Koha (`C4::SIP`), sobre as mesmas regras do balcão: o SIP2 é só um codec que traduz mensagens em comandos de circulação e recusas em respostas (doc 03, §1.2, item 11).

A tarefa tem duas partes, cada uma num PR:

- **4.5a, codec:** quadros, checksum, leitura das mensagens pedidas e escrita das respostas, sem rede (itens 1 a 15).
- **4.5b, servidor:** a conexão, as contas de terminal, o login e cada mensagem ligada à circulação, com os cenários do oráculo (itens 16 em diante).

## 1. Comportamento

### Quadros (4.5a)

1. Uma mensagem termina em CR (`\r`). Antes de ler, a mensagem é limpa como no Koha (`C4::SIP::SIPServer::read_request`): saem do começo os brancos e os caracteres que não são letra, dígito ou `[\]^_\``; saem do começo e do fim os caracteres de controle e brancos; saem os LF e CRLF do meio. Uma mensagem vazia depois da limpeza é respondida com `96`.
2. Uma mensagem tem no máximo 8 KiB antes do CR. Acima disso a conexão é fechada sem resposta (o Koha não tem limite; o Elzevir põe um, como em toda entrada externa).
3. Os bytes são lidos como UTF-8; um byte inválido vira U+FFFD no valor do campo, sem recusar a mensagem.
4. A resposta termina em CRLF, ou em CR com `terminator = "cr"` na conta do terminal (padrão do Koha: CRLF). As respostas fixas `96` (pedido de reenvio) e o reenvio literal da última resposta terminam sempre em CR, como no Koha.

### Detecção de erro (4.5a)

5. O checksum é o complemento de dois, em 16 bits, da soma dos bytes da mensagem até o `AZ` inclusive, escrito com quatro dígitos hexadecimais maiúsculos (`C4::SIP::Sip::Checksum`). Ex.: `12345` dá 65281 (`FF01`).
6. Uma mensagem com mais de 11 bytes cujo nono byte a partir do fim começa `AY` liga a detecção de erro na sessão: o número de sequência é o byte depois de `AY`, e o checksum depois de `AZ` é conferido. Checksum errado é respondido com `96AZFEF6` e a mensagem não é tratada. `97AZFEF5` também liga a detecção e pede o reenvio.
7. Com a detecção ligada, cada resposta ganha `AY<sequência>AZ<checksum>` no fim, com a sequência do pedido (0 quando o pedido não tinha). Uma mensagem sem `AY` com a detecção ligada desliga a detecção.
8. **Reenvio** (`97`): sem resposta anterior, responde `96`; com uma resposta anterior sem `AY`, repete-a literalmente; com `AY`, repete-a com a sequência e o checksum novos.

### Mensagens pedidas (4.5a)

9. O código da mensagem são os dois primeiros bytes. Os campos fixos seguem o modelo do Koha (`C4::SIP::Sip::MsgType`, `%handlers`), cortados pela largura de cada um, com os brancos do fim tirados (como o `unpack "A"` do Perl); uma mensagem mais curta que o modelo tem os campos que couberem e nenhum campo variável.
10. Depois dos campos fixos vêm os campos variáveis, separados por `|`, cada um com dois caracteres de código e o valor. Um código que a mensagem não conhece é ignorado; um código repetido mantém o primeiro valor.
11. O Elzevir lê estas mensagens:

    | Código | Mensagem | Campos fixos | Campos variáveis |
    |---|---|---|---|
    | `93` | Login | algoritmo do usuário (1), da senha (1) | `CN`, `CO`, `CP` |
    | `99` | Estado do terminal | estado (1), largura de impressão (3), versão do protocolo (4) | |
    | `97` | Reenvio | | |
    | `23` | Estado do leitor | idioma (3), data (18) | `AO`, `AA`, `AC`, `AD` |
    | `63` | Informações do leitor | idioma (3), data (18), resumo (10) | `AO`, `AA`, `AC`, `AD`, `BP`, `BQ` |
    | `17` | Informações do exemplar | data (18) | `AO`, `AB`, `AC` |
    | `11` | Empréstimo | renovação pelo terminal (1), sem bloqueio (1), data (18), vencimento sem bloqueio (18) | `AO`, `AA`, `AB`, `AC`, `CH`, `AD`, `BO`, `BI` |
    | `09` | Devolução | sem bloqueio (1), data (18), data de devolução (18) | `AP`, `AO`, `AB`, `AC`, `CH`, `BI` |
    | `29` | Renovação | terceiro (1), sem bloqueio (1), data (18), vencimento sem bloqueio (18) | `AO`, `AA`, `AD`, `AB`, `AJ`, `AC`, `CH`, `BO` |
    | `35` | Fim da sessão do leitor | data (18) | `AO`, `AA`, `AC`, `AD` |

12. **Versão do protocolo**: a sessão começa na 1.00. `93` passa para a 2.00; `99` passa para a versão do campo (`1.` dá 1.00, `2.` dá 2.00, outra dá 1.00). `93` é lido em qualquer versão (ele mesmo passa para a 2.00). Na 1.00, as outras mensagens só da 2.00 (`63`, `17`, `29`, `35`) não são lidas, e `11` e `09` leem só os campos da 1.00 (`AO`, `AA`, `AB`, `AC` e `AP`, `AO`, `AB`, `AC`).
13. Qualquer outra mensagem, ou uma mensagem fora da versão, é respondida com `96`. **Desvio**: o Koha também trata bloqueio do leitor (`01`), reserva (`15`), atualização do exemplar (`19`), habilitação do leitor (`25`), pagamento (`37`) e renovar tudo (`65`); o Elzevir v0 responde `96` a elas e não as anuncia no `BX` ([desvios conhecidos](../../desvios-conhecidos.md)).

### Respostas (4.5a)

14. Cada resposta é escrita na ordem de campos do Koha (`handle_*` de `C4::SIP::Sip::MsgType`):
    - um campo obrigatório sai mesmo vazio (`AF|`); um campo opcional vazio não sai;
    - CR, LF e CRLF dentro de um valor viram um espaço, e o `|` vira `&#124;`;
    - datas e horas saem como `AAAAMMDD    HHMMSS` (quatro espaços), no fuso da biblioteca;
    - o vencimento sai pelo dia, sempre com `235900`, como o Koha faz ao formatar a data de vencimento (`C4::SIP::Sip::timestamp`);
    - valores sim/não saem `Y`/`N`; as bandeiras de "negado" saem `Y` quando negado e branco quando não;
    - contagens saem com quatro dígitos (`0003`), e o tempo limite e as tentativas do `98` com três.
15. As respostas escritas são `94`, `98`, `24`, `64`, `18`, `12`, `10`, `30`, `36` e `96`, cada uma com os campos que o servidor da 4.5b preenche (itens 16 em diante).

### Servidor (4.5b)

16. O servidor SIP2 é ligado por `[sip2]` no `elzevir.toml` (endereço de escuta e as contas de terminal: usuário, senha, unidade e `terminator`). Sem a seção, não abre porta.
17. A primeira mensagem de uma conexão tem de ser `93`. Login recusado é respondido `940` e espera outro; uma primeira mensagem que não é `93` fecha a conexão sem resposta (`raw_transport` do Koha). Algoritmos de cifra diferentes de `0` são recusados.
18. `99` responde `98` com o que a unidade permite, a hora, a versão e, na 2.00, as mensagens aceitas (`BX`).
19. `23`, `63`, `17`, `11`, `09`, `29` e `35` usam as mesmas regras e as mesmas recusas do balcão de circulação; as mensagens de tela (`AF`) são as do Koha para cada recusa.
20. O log segue o da circulação, com a senha do terminal e a do leitor trocadas por `***` (`remove_password_from_message` do Koha).

## 2. Origem no Koha

- Quadros e checksum: `C4::SIP::SIPServer::read_request`, `C4::SIP::Sip::write_msg`, `C4::SIP::Sip::Checksum`.
- Mensagens: `C4::SIP::Sip::MsgType` (`%handlers`, `_initialize`, `handle`, `handle_*`, `send_acs_status`, `patron_status_string`).
- Campos: `C4::SIP::Sip::Constants`; formato: `C4::SIP::Sip::add_field`, `maybe_add`, `timestamp`, `add_count`, `sipbool`, `boolspace`, `denied`.
- Testes do Koha: `t/SIP/Sip.t` (checksum, data, campos) e `t/db_dependent/SIP/Message.t`.
- Diferenças intencionais: limite de 8 KiB por mensagem; delimitador fixo `|` (o Koha deixa configurar); sem `hide_fields`, `allow_fields`, modelos de campo (`ae_field_template`) e campos próprios (`custom_patron_fields`) nesta tarefa; as mensagens do item 13.

## 3. Paridade

Exigida (doc 04, §2). O codec é testado com mensagens tiradas dos testes do Koha e do padrão; o servidor, com cenários gravados do oráculo com o SIP ligado, nunca a rede.

## 4. Fora desta tarefa

- SIP sobre TLS e o transporte telnet do Koha.
- As mensagens do item 13 e o pagamento de multas pelo terminal.
- Codificação de saída diferente de UTF-8 (`encoding` do Koha).
