use std::borrow::Cow;

// Backend messages are kept in their original language until displayed, so the
// language selector can also update messages that are already on screen.
pub fn english(message: &str) -> Cow<'_, str> {
    let translated = match message {
        "Aguarda a conclusão do pedido anterior." => "Wait for the previous request to finish.",
        "Ainda sem faixa" => "No track yet",
        "À espera da tua música" => "Waiting for your music",
        "Escolhe o que queres ouvir" => "Choose something to play",
        "Sem dispositivo" => "No device",
        "Nenhum dispositivo ativo" => "No active device",
        "Dispositivo de exemplo" => "Demo device",
        "Soloist · este computador" => "Soloist · this computer",
        "Liga o Soloist ou experimenta a demonstração." => "Connect Soloist or try the demo.",
        "Sem playlists nesta página." => "No playlists on this page.",
        "Não foi possível iniciar a ligação segura ao Spotify." => "Could not initialise a secure connection to Spotify.",
        "A iniciar o Soloist…" => "Starting Soloist…",
        "A chave guardada do Soloist foi removida." => "The saved Soloist key was removed.",
        "Não foi possível remover a chave guardada." => "Could not remove the saved key.",
        "A autorização guardada foi removida deste PC." => "The saved authorisation was removed from this computer.",
        "Não foi possível remover toda a autorização do cofre do sistema." => "Could not remove all authorisation data from the system keyring.",
        "Autorização da pesquisa recuperada." => "Search authorisation restored.",
        "Autoriza a biblioteca no navegador." => "Authorise the library in your browser.",
        "A carregar a biblioteca…" => "Loading the library…",
        "Biblioteca autorizada." => "Library authorised.",
        "Biblioteca autorizada. Não foi possível guardá-la no cofre do sistema." => "Library authorised. Could not save the authorisation to the system keyring.",
        "Autorização cancelada." => "Authorisation cancelled.",
        "O Spotify não dá acesso às faixas desta playlist. Podes abri-la no Spotify." => "Spotify does not provide access to this playlist’s tracks. You can open it in Spotify.",
        "Autoriza a biblioteca em Ligações." => "Authorise the library in Connections.",
        "Sem faixas disponíveis nesta página." => "No tracks available on this page.",
        "Autoriza o catálogo no navegador." => "Authorise the catalogue in your browser.",
        "Catálogo autorizado. Já podes pesquisar faixas." => "Catalogue authorised. You can now search for tracks.",
        "Catálogo autorizado. Não foi possível guardá-lo no cofre do sistema." => "Catalogue authorised. Could not save the authorisation to the system keyring.",
        "Escreve um nome ou artista com até 160 caracteres." => "Enter a track or artist name up to 160 characters long.",
        "Autoriza o catálogo em Ligações para pesquisar por nome." => "Authorise the catalogue in Connections to search by name.",
        "Não foram encontradas faixas." => "No tracks found.",
        "1 faixa encontrada" => "1 track found",
        "Liga um dispositivo para veres a fila." => "Connect a device to see the queue.",
        "A fila está vazia." => "The queue is empty.",
        "Escolhe uma faixa para a começar a ouvir agora." => "Choose a track to play it now.",
        "Escolhe uma das faixas de exemplo." => "Choose one of the sample tracks.",
        "Cola um link de faixa do Spotify ou um URI spotify:track:…" => "Paste a Spotify track link or a spotify:track:… URI.",
        "Pedido enviado ao Soloist. A atualizar a reprodução…" => "Request sent to Soloist. Updating playback…",
        "Soloist ligado · reprodução local neste computador" => "Soloist connected · playing on this computer",
        "A recuperar a autorização guardada." => "Restoring saved authorisation.",
        "Autoriza a ligação no navegador. Podes cancelar aqui." => "Authorise the connection in your browser. You can cancel here.",
        "Início de sessão cancelado." => "Sign-in cancelled.",
        "Reprodução desligada." => "Playback disconnected.",
        "Demonstração · dados fictícios · sem áudio ou pedidos à API" => "Demo · sample data · no audio or API requests",
        "Liga o Soloist ou um dispositivo Spotify para ouvir a faixa." => "Connect Soloist or a Spotify device to play the track.",
        "Ligado ao Spotify · áudio no dispositivo selecionado" => "Connected to Spotify · audio on the selected device",
        "Abre o Spotify num dispositivo e inicia uma faixa. Depois, atualiza." => "Open Spotify on a device, start a track, then refresh.",
        "Pedido enviado. A confirmar o estado no Spotify…" => "Request sent. Confirming playback state in Spotify…",
        "Não foi possível abrir o navegador." => "Could not open the browser.",
        "A sessão expirou. Volta a iniciar sessão." => "The session expired. Sign in again.",
        "Acesso recusado. Confirma Premium, permissões e utilizadores autorizados no painel Spotify." => "Access denied. Check Premium, permissions and authorised users in the Spotify dashboard.",
        "Não existe um dispositivo disponível. Abre o Spotify num dispositivo e inicia uma faixa." => "No device is available. Open Spotify on a device and start a track.",
        "A quota da aplicação foi esgotada. Consulta o painel Spotify; as atualizações estão suspensas." => "The app quota has been reached. Check the Spotify dashboard. Updates are paused.",
        "Sem ligação ao Spotify. Verifica a rede e tenta atualizar." => "Could not reach Spotify. Check the network and try refreshing.",
        "A resposta do Spotify não é válida ou excede o limite de memória." => "The Spotify response is invalid or exceeds the memory limit.",
        "O Client ID deve conter 32 caracteres hexadecimais. Não uses o Client Secret." => "The Client ID must contain 32 hexadecimal characters. Use the Client ID from your dashboard.",
        "Não foi possível usar a porta local 8888. Fecha outra instância e tenta novamente." => "Could not use local port 8888. Close the other instance and try again.",
        "Não foi possível abrir o navegador predefinido." => "Could not open the default browser.",
        "O início de sessão foi cancelado." => "Sign-in was cancelled.",
        "O início de sessão expirou. Tenta novamente." => "Sign-in timed out. Try again.",
        "A autorização foi recusada no Spotify." => "Authorisation was declined in Spotify.",
        "A autorização guardada expirou. Volta a autorizar em Ligações." => "The saved authorisation expired. Authorise again in Connections.",
        "Não foi possível contactar o serviço de autenticação. Verifica a ligação." => "Could not reach the authentication service. Check your connection.",
        "A resposta de autenticação não é válida." => "The authentication response is invalid.",
        crate::soloist::PAIR_REQUIRED => "Select Soloist in the Spotify app’s device list to pair your account.",
        "Usa um endereço local como 127.0.0.1:9090." => "Use a local address such as 127.0.0.1:9090.",
        "O Soloist só pode ser ligado por loopback, com uma porta válida." => "Connect Soloist using a loopback address and a valid port.",
        "Soloist indisponível. Inicia-o com --ws 127.0.0.1:9090." => "Soloist is unavailable. Start it with --ws 127.0.0.1:9090.",
        "Erro de ligação local." => "Local connection error.",
        "O serviço local não respondeu como Soloist WebSocket." => "The local service did not respond as a Soloist WebSocket.",
        "Falhou o envio ao Soloist." => "Could not send the request to Soloist.",
        "O Soloist desligou-se ou não respondeu. Tenta voltar a ligar." => "Soloist disconnected or did not respond. Try reconnecting.",
        "Resposta inválida do Soloist." => "Invalid response from Soloist.",
        "O Soloist recusou o pedido. Confirma o emparelhamento e a ação disponível." => "Soloist rejected the request. Check pairing and available actions.",
        "A resposta do Soloist excedeu o tempo limite." => "Soloist did not respond in time.",
        "A porta está ocupada. Confirma o endereço do Soloist em Ligações." => "The port is in use. Check the Soloist address in Connections.",
        "Para trocar a chave guardada, usa «Esquecer chave» primeiro." => "Choose Forget key before replacing the saved key.",
        "Chave guardada. Será usada no próximo arranque do Soloist." => "Key saved. It will be used the next time Soloist starts.",
        "Não foi possível guardar a chave no cofre do sistema." => "Could not save the key to the system keyring.",
        "Introduz a tua chave Soloist para iniciar a reprodução local." => "Enter your Soloist key to start local playback.",
        "Arranque do Soloist cancelado." => "Soloist startup cancelled.",
        "Não foi possível verificar o processo Soloist." => "Could not check the Soloist process.",
        "Esta versão do Soloist expirou. Descarrega a versão oficial mais recente." => "This version of Soloist expired. Download the latest official version.",
        "O Soloist arrancou, mas não foi possível guardar a chave no cofre do sistema." => "Soloist started. Could not save the key to the system keyring.",
        "O Soloist não ficou disponível. Verifica a chave e se a porta está livre." => "Soloist did not become available. Check the key and whether the port is free.",
        _ => return formatted_message(message),
    };
    Cow::Borrowed(translated)
}

fn formatted_message(message: &str) -> Cow<'_, str> {
    for (prefix, suffix, translated_prefix, translated_suffix) in [
        ("", " faixas encontradas", "", " tracks found"),
        (
            "O Spotify pediu uma pausa. Tenta novamente dentro de ",
            " segundos.",
            "Spotify asked you to wait. Try again in ",
            " seconds.",
        ),
        (
            "O Spotify devolveu um erro (HTTP ",
            ").",
            "Spotify returned an error (HTTP ",
            ").",
        ),
        (
            "O Spotify recusou a autenticação (HTTP ",
            "). Verifica o Client ID e o endereço de retorno.",
            "Spotify rejected authentication (HTTP ",
            "). Check the Client ID and redirect URI.",
        ),
        (
            "Indica o caminho completo do executável oficial ",
            ".",
            "Enter the full path to the official ",
            " executable.",
        ),
        (
            "Não foi possível iniciar o Soloist: ",
            "",
            "Could not start Soloist: ",
            "",
        ),
        (
            "O Soloist terminou ao iniciar (",
            "). Verifica a chave, a versão e se já existe outra instância.",
            "Soloist exited on startup (",
            "). Check the key, version and whether another instance is running.",
        ),
    ] {
        if let Some(detail) = message
            .strip_prefix(prefix)
            .and_then(|value| value.strip_suffix(suffix))
        {
            return Cow::Owned(format!("{translated_prefix}{detail}{translated_suffix}"));
        }
    }
    if let Some(status) =
        message.strip_suffix(" Não foi possível guardar a autorização no cofre do sistema.")
    {
        return Cow::Owned(format!(
            "{} Could not save authorisation to the system keyring.",
            english(status)
        ));
    }
    for note in [
        "Para trocar a chave guardada, usa «Esquecer chave» primeiro.",
        "Chave guardada. Será usada no próximo arranque do Soloist.",
        "Não foi possível guardar a chave no cofre do sistema.",
        "O Soloist arrancou, mas não foi possível guardar a chave no cofre do sistema.",
    ] {
        if let Some(status) = message
            .strip_suffix(note)
            .and_then(|value| value.strip_suffix(' '))
        {
            return Cow::Owned(format!("{} {}", english(status), english(note)));
        }
    }
    Cow::Borrowed(message)
}
