use anyhow::Result;
use knyst::{
    audio_backend::JackBackend,
    controller::print_error_handler,
    envelope::Envelope,
    graph,
    handles::{graph_output, handle, Handle},
    modal_interface::commands,
    prelude::*,
    sphere::{KnystSphere, SphereSettings},
    trig::interval_trig,
};
use knyst_reverb::luff_verb;
use rand::{thread_rng, Rng};
fn main() -> Result<()> {
    // let mut backend = CpalBackend::new(CpalBackendOptions::default())?;
    let mut backend = JackBackend::new("Knyst<3JACK")?;
    let _sphere = KnystSphere::start(
        &mut backend,
        SphereSettings {
            num_inputs: 1,
            num_outputs: 2,
            ..Default::default()
        },
        print_error_handler,
    );

    let mut rng = thread_rng();
    commands().init_local_graph(commands().default_graph_settings());
    for freq in [300, 400, 500, 600, 700, 800].iter() {
        let sig = sine().freq(*freq as f32).out("sig") * 0.25;
        let env = Envelope {
            points: vec![(1.0, 0.001), (0.0, 0.2)],
            ..Default::default()
        };
        let sig = sig
            * handle(env.to_gen()).set(
                "restart",
                interval_trig().interval(rng.gen_range(1.5f32..3.5)),
            );
        graph_output(0, sig);
    }
    let sig = commands().upload_local_graph();
    let verb = luff_verb(2350 * 48, 0.65).lowpass(7000.).damping(4000.);
    // .input(sig * 0.125);
    // .input(sig * 0.125 + graph_input(0, 1));
    verb.input(graph_input(0, 1));
    // verb.input(sig * 0.125);
    let sig = verb * 0.5;
    // let sig = verb * 0.25 + (sig * 0.25);
    graph_output(0, sig.repeat_outputs(1));

    // std::thread::sleep(std::time::Duration::from_millis(150));
    // for &freq in [400, 600, 500].iter().cycle() {
    //     // new graph
    //     commands().init_local_graph(commands().default_graph_settings());
    //     let sig = sine().freq(freq as f32).out("sig") * 0.25;
    //     let env = Envelope {
    //         points: vec![(1.0, 0.005), (0.0, 0.5)],
    //         stop_action: StopAction::FreeGraph,
    //         ..Default::default()
    //     };
    //     let sig = sig * handle(env.to_gen());

    //     graph_output(0, sig.repeat_outputs(1));
    //     // push graph to sphere
    // let graph = commands().upload_local_graph();

    //     // graph_output(0, graph);
    //     verb.input(graph.out(0) * 1.1);
    //     std::thread::sleep(std::time::Duration::from_millis(2500));
    // }

    // graph_output(0, (sine(wt).freq(200.)).repeat_outputs(1));

    // let inspection = commands().request_inspection();
    // let inspection = inspection.recv().unwrap();
    // dbg!(inspection);

    let mut input = String::new();
    loop {
        match std::io::stdin().read_line(&mut input) {
            Ok(n) => {
                println!("{} bytes read", n);
                println!("{}", input.trim());
                let input = input.trim();
                if let Ok(freq) = input.parse::<usize>() {
                    // node0.freq(freq as f32);
                } else if input == "q" {
                    break;
                }
            }
            Err(error) => println!("error: {}", error),
        }
        input.clear();
    }
    Ok(())
}

// fn sine() -> NodeHandle<WavetableOscillatorOwnedHandle> {
//     wavetable_oscillator_owned(Wavetable::sine())
// }
fn sine() -> Handle<OscillatorHandle> {
    oscillator(WavetableId::cos())
}
