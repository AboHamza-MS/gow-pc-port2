// GOW-Port: exercise the CLI's real comparison, without duplicating its logic.
#define main gow_replay_cli_main
#define wmain gow_replay_cli_wmain
#include "../tools/render/replay_gs.cpp"
#undef main
#undef wmain

int main() {
    PassResult before,after;
    before.vram.resize(PS2_GS_VRAM_SIZE); after.vram=before.vram;
    if(!comparePasses(2,before,after,false)) return 1;
    std::vector<uint8_t> intermediate(PS2_GS_VRAM_SIZE);
    if(!checkpoint(before,7,5,replay::Op::Sync,intermediate)) return 1;
    intermediate[65540]=1;
    if(!checkpoint(after,7,5,replay::Op::Sync,intermediate)) return 1;
    // The End is identical: the intermediate change must be enough to reject parity.
    if(comparePasses(2,before,after,false)) return 1;
    after.checkpoints=before.checkpoints;
    if(!comparePasses(2,before,after,false)) return 1;
    after.checkpoints.clear();
    if(comparePasses(2,before,after,false)) return 1;
    after.checkpoints=before.checkpoints;
    after.checkpoints[0].record++;
    if(comparePasses(2,before,after,false)) return 1;
    after.checkpoints=before.checkpoints;
    after.checkpoints[0].op=replay::Op::Flush;
    if(comparePasses(2,before,after,false)) return 1;
    after.checkpoints=before.checkpoints;
    after.checkpoints[0].draws++;
    if(comparePasses(2,before,after,false)) return 1;
    after.checkpoints.clear();
    intermediate.resize(1);
    if(checkpoint(after,7,5,replay::Op::Sync,intermediate) || !after.checkpoints.empty()) return 1;
    std::cout<<"GS comparison: intermediate variation with an identical End and verified metadata: OK\n";
    return 0;
}
